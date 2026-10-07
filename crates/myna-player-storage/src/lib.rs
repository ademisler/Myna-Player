use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::UNIX_EPOCH,
};

use myna_player_core::{AppSettingsV1, CueStatus, SubtitleCue, TranscriptSegment};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use thiserror::Error;

const DATABASE_VERSION: i64 = 2;
const SAMPLE_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("storage lock was poisoned")]
    Poisoned,
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("settings serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("media identity failed: {0}")]
    MediaIdentity(#[from] std::io::Error),
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("database schema version {found} is newer than supported version {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaIdentity {
    pub fingerprint: String,
    pub canonical_path: String,
    pub size_bytes: u64,
    pub modified_ms: u64,
}

pub struct Storage {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut connection)?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_settings(&self) -> Result<AppSettingsV1, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let raw = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'app'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        raw.map(|value| serde_json::from_str(&value))
            .transpose()
            .map(|settings| settings.unwrap_or_default())
            .map_err(StorageError::from)
    }

    pub fn save_settings(&self, settings: &AppSettingsV1) -> Result<(), StorageError> {
        settings.validate().map_err(StorageError::InvalidSettings)?;
        let raw = serde_json::to_string(settings)?;
        let mut connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO settings(key, value_json, updated_at)
             VALUES('app', ?1, unixepoch())
             ON CONFLICT(key) DO UPDATE SET
               value_json = excluded.value_json,
               updated_at = excluded.updated_at",
            [raw],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_media(
        &self,
        identity: &MediaIdentity,
        duration_ms: u64,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "INSERT INTO media(fingerprint, canonical_path, size_bytes, modified_ms, duration_ms, last_opened_at)
             VALUES(?1, ?2, ?3, ?4, ?5, unixepoch())
             ON CONFLICT(fingerprint) DO UPDATE SET
               canonical_path = excluded.canonical_path,
               duration_ms = excluded.duration_ms,
               last_opened_at = excluded.last_opened_at",
            params![
                identity.fingerprint,
                identity.canonical_path,
                identity.size_bytes,
                identity.modified_ms,
                duration_ms
            ],
        )?;
        Ok(())
    }

    pub fn set_media_cache_policy(
        &self,
        fingerprint: &str,
        keep_completed_transcripts: bool,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "UPDATE media SET cache_policy = ?2 WHERE fingerprint = ?1",
            params![
                fingerprint,
                if keep_completed_transcripts {
                    "persistent"
                } else {
                    "ephemeral"
                }
            ],
        )?;
        Ok(())
    }

    pub fn purge_ephemeral_cache(&self) -> Result<usize, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let deleted =
            connection.execute("DELETE FROM media WHERE cache_policy = 'ephemeral'", [])?;
        Ok(deleted)
    }

    pub fn purge_media_cache(&self, fingerprint: &str) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute("DELETE FROM media WHERE fingerprint = ?1", [fingerprint])?;
        Ok(())
    }

    pub fn cache_usage_bytes(&self) -> Result<u64, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let bytes: i64 = connection.query_row(
            "SELECT
               COALESCE((SELECT SUM(LENGTH(text) + 128) FROM transcript_segments), 0) +
               COALESCE((SELECT SUM(LENGTH(COALESCE(translated_text, '')) + 96) FROM translations), 0) +
               COALESCE((SELECT COUNT(*) * 128 FROM processing_windows), 0)",
            [],
            |row| row.get(0),
        )?;
        Ok(bytes.max(0) as u64)
    }

    pub fn enforce_cache_limit(
        &self,
        max_bytes: u64,
        preserve_fingerprint: Option<&str>,
    ) -> Result<Vec<String>, StorageError> {
        let mut removed = Vec::new();
        loop {
            if self.cache_usage_bytes()? <= max_bytes {
                break;
            }
            let candidate = {
                let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
                connection
                    .query_row(
                        "SELECT fingerprint FROM media
                         WHERE (?1 IS NULL OR fingerprint <> ?1)
                         ORDER BY CASE cache_policy WHEN 'ephemeral' THEN 0 ELSE 1 END,
                                  last_opened_at ASC
                         LIMIT 1",
                        [preserve_fingerprint],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
            };
            let Some(fingerprint) = candidate else { break };
            self.purge_media_cache(&fingerprint)?;
            removed.push(fingerprint);
        }
        Ok(removed)
    }

    pub fn save_playback_position(
        &self,
        fingerprint: &str,
        position_ms: u64,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "UPDATE media SET playback_position_ms = ?2 WHERE fingerprint = ?1",
            params![fingerprint, position_ms],
        )?;
        Ok(())
    }

    pub fn playback_position(&self, fingerprint: &str) -> Result<u64, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        Ok(connection
            .query_row(
                "SELECT playback_position_ms FROM media WHERE fingerprint = ?1",
                [fingerprint],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn completed_windows(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
    ) -> Result<Vec<(u64, u64)>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare(
            "SELECT start_ms, end_ms
             FROM processing_windows
             WHERE fingerprint = ?1 AND audio_track = ?2
               AND pipeline_version = ?3 AND status = 'completed'
             ORDER BY start_ms",
        )?;
        let rows = statement
            .query_map(params![fingerprint, audio_track, pipeline_version], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn mark_window_running(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        start_ms: u64,
        end_ms: u64,
        generation: u64,
    ) -> Result<(), StorageError> {
        self.write_window_status(
            fingerprint,
            audio_track,
            pipeline_version,
            start_ms,
            end_ms,
            generation,
            "running",
            None,
        )
    }

    pub fn mark_window_completed(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        start_ms: u64,
        end_ms: u64,
        generation: u64,
    ) -> Result<(), StorageError> {
        self.write_window_status(
            fingerprint,
            audio_track,
            pipeline_version,
            start_ms,
            end_ms,
            generation,
            "completed",
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn mark_window_failed(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        start_ms: u64,
        end_ms: u64,
        generation: u64,
        error: &str,
    ) -> Result<(), StorageError> {
        self.write_window_status(
            fingerprint,
            audio_track,
            pipeline_version,
            start_ms,
            end_ms,
            generation,
            "failed",
            Some(error),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn write_window_status(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        start_ms: u64,
        end_ms: u64,
        generation: u64,
        status: &str,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        connection.execute(
            "INSERT INTO processing_windows(
               fingerprint, audio_track, pipeline_version, start_ms, end_ms,
               generation, status, error, updated_at
             )
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())
             ON CONFLICT(fingerprint, audio_track, pipeline_version, start_ms, end_ms)
             DO UPDATE SET
               generation = excluded.generation,
               status = excluded.status,
               error = excluded.error,
               updated_at = excluded.updated_at",
            params![
                fingerprint,
                audio_track,
                pipeline_version,
                start_ms,
                end_ms,
                generation,
                status,
                error
            ],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn replace_window_segments(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        start_ms: u64,
        end_ms: u64,
        generation: u64,
        segments: &[TranscriptSegment],
    ) -> Result<(), StorageError> {
        let mut connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM transcript_segments
             WHERE fingerprint = ?1 AND audio_track = ?2 AND pipeline_version = ?3
               AND start_ms < ?5 AND end_ms > ?4",
            params![fingerprint, audio_track, pipeline_version, start_ms, end_ms],
        )?;
        insert_transcript_segments(
            &transaction,
            fingerprint,
            audio_track,
            pipeline_version,
            segments,
        )?;
        transaction.execute(
            "INSERT INTO processing_windows(
               fingerprint, audio_track, pipeline_version, start_ms, end_ms,
               generation, status, error, updated_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'completed', NULL, unixepoch())
             ON CONFLICT(fingerprint, audio_track, pipeline_version, start_ms, end_ms)
             DO UPDATE SET generation = excluded.generation, status = 'completed',
                           error = NULL, updated_at = excluded.updated_at",
            params![
                fingerprint,
                audio_track,
                pipeline_version,
                start_ms,
                end_ms,
                generation
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn invalidate_translations_for_segment(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        segment_id: &str,
    ) -> Result<usize, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let count = connection.execute(
            "DELETE FROM translations
             WHERE fingerprint = ?1 AND audio_track = ?2
               AND pipeline_version = ?3 AND segment_id = ?4",
            params![fingerprint, audio_track, pipeline_version, segment_id],
        )?;
        Ok(count)
    }

    pub fn store_transcript_segments(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        segments: &[TranscriptSegment],
    ) -> Result<(), StorageError> {
        let mut connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let transaction = connection.transaction()?;
        insert_transcript_segments(
            &transaction,
            fingerprint,
            audio_track,
            pipeline_version,
            segments,
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn load_transcript_segments(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
    ) -> Result<Vec<TranscriptSegment>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare(
            "SELECT segment_id, start_ms, end_ms, text, detected_language,
                    language_confidence, is_final
             FROM transcript_segments
             WHERE fingerprint = ?1 AND audio_track = ?2 AND pipeline_version = ?3
             ORDER BY start_ms, end_ms, segment_id",
        )?;
        let rows =
            statement.query_map(params![fingerprint, audio_track, pipeline_version], |row| {
                Ok(TranscriptSegment {
                    id: row.get(0)?,
                    start_ms: row.get(1)?,
                    end_ms: row.get(2)?,
                    text: row.get(3)?,
                    detected_language: row.get(4)?,
                    language_confidence: row.get(5)?,
                    is_final: row.get(6)?,
                })
            })?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn store_translations(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        provider_id: &str,
        target_language: &str,
        cues: &[SubtitleCue],
    ) -> Result<(), StorageError> {
        let mut connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO translations(
                   fingerprint, audio_track, pipeline_version, provider_id,
                   target_language, segment_id, translated_text, status
                 )
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(
                   fingerprint, audio_track, pipeline_version, provider_id,
                   target_language, segment_id
                 )
                 DO UPDATE SET
                   translated_text = excluded.translated_text,
                   status = excluded.status",
            )?;
            for cue in cues {
                statement.execute(params![
                    fingerprint,
                    audio_track,
                    pipeline_version,
                    provider_id,
                    target_language,
                    cue.id,
                    cue.translated_text,
                    cue_status(cue.status)
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn load_translated_cues(
        &self,
        fingerprint: &str,
        audio_track: u32,
        pipeline_version: &str,
        provider_id: &str,
        target_language: &str,
    ) -> Result<Vec<SubtitleCue>, StorageError> {
        let connection = self.connection.lock().map_err(|_| StorageError::Poisoned)?;
        let mut statement = connection.prepare(
            "SELECT s.segment_id, s.start_ms, s.end_ms, s.text,
                    t.translated_text, s.detected_language, t.target_language
             FROM transcript_segments s
             JOIN translations t
               ON t.fingerprint = s.fingerprint
              AND t.audio_track = s.audio_track
              AND t.pipeline_version = s.pipeline_version
              AND t.segment_id = s.segment_id
             WHERE s.fingerprint = ?1 AND s.audio_track = ?2
               AND s.pipeline_version = ?3 AND t.provider_id = ?4
               AND t.target_language = ?5
             ORDER BY s.start_ms, s.end_ms",
        )?;
        let rows = statement.query_map(
            params![
                fingerprint,
                audio_track,
                pipeline_version,
                provider_id,
                target_language
            ],
            |row| {
                Ok(SubtitleCue {
                    id: row.get(0)?,
                    start_ms: row.get(1)?,
                    end_ms: row.get(2)?,
                    source_text: row.get(3)?,
                    translated_text: row.get(4)?,
                    source_language: row.get(5)?,
                    target_language: row.get(6)?,
                    status: CueStatus::Ready,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }
}

pub fn media_identity(path: impl AsRef<Path>) -> Result<MediaIdentity, StorageError> {
    let canonical = std::fs::canonicalize(path)?;
    let metadata = canonical.metadata()?;
    let size_bytes = metadata.len();
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0);
    let mut file = File::open(&canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(size_bytes.to_le_bytes());
    hasher.update(modified_ms.to_le_bytes());

    let head_len = size_bytes.min(SAMPLE_BYTES) as usize;
    let mut head = vec![0; head_len];
    file.read_exact(&mut head)?;
    hasher.update(&head);

    if size_bytes > SAMPLE_BYTES {
        let tail_start = size_bytes.saturating_sub(SAMPLE_BYTES);
        file.seek(SeekFrom::Start(tail_start))?;
        let mut tail = vec![0; (size_bytes - tail_start) as usize];
        file.read_exact(&mut tail)?;
        hasher.update(&tail);
    }

    Ok(MediaIdentity {
        fingerprint: format!("{:x}", hasher.finalize()),
        canonical_path: canonical.to_string_lossy().into_owned(),
        size_bytes,
        modified_ms,
    })
}

fn cue_status(status: CueStatus) -> &'static str {
    match status {
        CueStatus::Queued => "queued",
        CueStatus::Transcribing => "transcribing",
        CueStatus::Transcribed => "transcribed",
        CueStatus::Translating => "translating",
        CueStatus::Ready => "ready",
        CueStatus::Failed => "failed",
    }
}

fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_meta(
           key TEXT PRIMARY KEY,
           value INTEGER NOT NULL
         );",
    )?;
    let current = connection
        .query_row(
            "SELECT value FROM schema_meta WHERE key = 'version'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    if current > DATABASE_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: current,
            supported: DATABASE_VERSION,
        });
    }

    let transaction = connection.transaction()?;
    if current == 0 {
        create_schema_v2(&transaction)?;
    } else if current == 1 {
        if !column_exists(&transaction, "media", "cache_policy")? {
            transaction.execute(
                "ALTER TABLE media ADD COLUMN cache_policy TEXT NOT NULL DEFAULT 'persistent'",
                [],
            )?;
        }
        transaction.execute_batch(
            "CREATE INDEX IF NOT EXISTS media_last_opened
               ON media(cache_policy, last_opened_at);
             CREATE INDEX IF NOT EXISTS processing_status
               ON processing_windows(fingerprint, audio_track, pipeline_version, status, start_ms);",
        )?;
    }
    transaction.execute(
        "INSERT INTO schema_meta(key, value) VALUES('version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [DATABASE_VERSION],
    )?;
    transaction.commit()?;
    let integrity: String = connection.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(StorageError::Database(rusqlite::Error::InvalidQuery));
    }
    Ok(())
}

fn create_schema_v2(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings(
           key TEXT PRIMARY KEY,
           value_json TEXT NOT NULL,
           updated_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS media(
           fingerprint TEXT PRIMARY KEY,
           canonical_path TEXT NOT NULL,
           size_bytes INTEGER NOT NULL,
           modified_ms INTEGER NOT NULL,
           duration_ms INTEGER NOT NULL,
           playback_position_ms INTEGER NOT NULL DEFAULT 0,
           last_opened_at INTEGER NOT NULL,
           cache_policy TEXT NOT NULL DEFAULT 'persistent'
         );
         CREATE TABLE IF NOT EXISTS processing_windows(
           fingerprint TEXT NOT NULL,
           audio_track INTEGER NOT NULL,
           pipeline_version TEXT NOT NULL,
           start_ms INTEGER NOT NULL,
           end_ms INTEGER NOT NULL,
           generation INTEGER NOT NULL,
           status TEXT NOT NULL,
           error TEXT,
           updated_at INTEGER NOT NULL,
           PRIMARY KEY(fingerprint, audio_track, pipeline_version, start_ms, end_ms),
           FOREIGN KEY(fingerprint) REFERENCES media(fingerprint) ON DELETE CASCADE
         );
         CREATE TABLE IF NOT EXISTS transcript_segments(
           fingerprint TEXT NOT NULL,
           audio_track INTEGER NOT NULL,
           pipeline_version TEXT NOT NULL,
           segment_id TEXT NOT NULL,
           start_ms INTEGER NOT NULL,
           end_ms INTEGER NOT NULL,
           text TEXT NOT NULL,
           detected_language TEXT,
           language_confidence REAL,
           is_final INTEGER NOT NULL,
           PRIMARY KEY(fingerprint, audio_track, pipeline_version, segment_id),
           FOREIGN KEY(fingerprint) REFERENCES media(fingerprint) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS transcript_timeline
           ON transcript_segments(fingerprint, audio_track, pipeline_version, start_ms);
         CREATE TABLE IF NOT EXISTS translations(
           fingerprint TEXT NOT NULL,
           audio_track INTEGER NOT NULL,
           pipeline_version TEXT NOT NULL,
           provider_id TEXT NOT NULL,
           target_language TEXT NOT NULL,
           segment_id TEXT NOT NULL,
           translated_text TEXT,
           status TEXT NOT NULL,
           PRIMARY KEY(
             fingerprint, audio_track, pipeline_version,
             provider_id, target_language, segment_id
           ),
           FOREIGN KEY(fingerprint, audio_track, pipeline_version, segment_id)
             REFERENCES transcript_segments(
               fingerprint, audio_track, pipeline_version, segment_id
             ) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS media_last_opened
           ON media(cache_policy, last_opened_at);
         CREATE INDEX IF NOT EXISTS processing_status
           ON processing_windows(fingerprint, audio_track, pipeline_version, status, start_ms);",
    )
}

fn column_exists(
    connection: &Connection,
    table: &str,
    column: &str,
) -> Result<bool, rusqlite::Error> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement.query_map([], |row| row.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn insert_transcript_segments(
    connection: &Connection,
    fingerprint: &str,
    audio_track: u32,
    pipeline_version: &str,
    segments: &[TranscriptSegment],
) -> Result<(), rusqlite::Error> {
    let mut statement = connection.prepare(
        "INSERT INTO transcript_segments(
           fingerprint, audio_track, pipeline_version, segment_id,
           start_ms, end_ms, text, detected_language, language_confidence, is_final
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(fingerprint, audio_track, pipeline_version, segment_id)
         DO UPDATE SET start_ms = excluded.start_ms, end_ms = excluded.end_ms,
                       text = excluded.text, detected_language = excluded.detected_language,
                       language_confidence = excluded.language_confidence,
                       is_final = excluded.is_final",
    )?;
    for segment in segments {
        statement.execute(params![
            fingerprint,
            audio_track,
            pipeline_version,
            segment.id,
            segment.start_ms,
            segment.end_ms,
            segment.text,
            segment.detected_language,
            segment.language_confidence,
            segment.is_final,
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use tempfile::tempdir;

    use super::*;

    fn test_storage() -> (tempfile::TempDir, Storage) {
        let directory = tempdir().unwrap();
        let storage = Storage::open(directory.path().join("myna-player.sqlite3")).unwrap();
        (directory, storage)
    }

    fn insert_media(storage: &Storage, fingerprint: &str) {
        storage
            .upsert_media(
                &MediaIdentity {
                    fingerprint: fingerprint.into(),
                    canonical_path: "/tmp/video.mp4".into(),
                    size_bytes: 10,
                    modified_ms: 1,
                },
                60_000,
            )
            .unwrap();
    }

    #[test]
    fn settings_save_is_validated_and_persistent() {
        let (_directory, storage) = test_storage();
        let mut settings = AppSettingsV1::default();
        settings.general.preferred_subtitle_language = "DE".into();
        storage.save_settings(&settings).unwrap();

        let loaded = storage.load_settings().unwrap();
        assert_eq!(loaded.general.preferred_subtitle_language, "DE");

        settings.transcription.chunk_duration_ms = 100;
        assert!(matches!(
            storage.save_settings(&settings),
            Err(StorageError::InvalidSettings(_))
        ));
        assert_eq!(
            storage
                .load_settings()
                .unwrap()
                .general
                .preferred_subtitle_language,
            "DE"
        );
    }

    #[test]
    fn source_and_translation_are_stored_separately() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "media");
        let segment = TranscriptSegment {
            id: "segment-1".into(),
            start_ms: 1_000,
            end_ms: 2_000,
            text: "Hello".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        };
        storage
            .store_transcript_segments("media", 0, "v1", std::slice::from_ref(&segment))
            .unwrap();
        storage
            .store_translations(
                "media",
                0,
                "v1",
                "deepl",
                "TR",
                &[SubtitleCue {
                    id: segment.id.clone(),
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    source_text: segment.text.clone(),
                    translated_text: Some("Merhaba".into()),
                    source_language: Some("en".into()),
                    target_language: Some("TR".into()),
                    status: CueStatus::Ready,
                }],
            )
            .unwrap();

        let sources = storage.load_transcript_segments("media", 0, "v1").unwrap();
        let translations = storage
            .load_translated_cues("media", 0, "v1", "deepl", "TR")
            .unwrap();
        assert_eq!(sources[0].text, "Hello");
        assert_eq!(translations[0].translated_text.as_deref(), Some("Merhaba"));
    }

    #[test]
    fn processing_checkpoint_survives_reopen() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("myna-player.sqlite3");
        {
            let storage = Storage::open(&database).unwrap();
            insert_media(&storage, "media");
            storage
                .mark_window_completed("media", 0, "v1", 0, 30_000, 1)
                .unwrap();
        }
        let reopened = Storage::open(&database).unwrap();
        assert_eq!(
            reopened.completed_windows("media", 0, "v1").unwrap(),
            vec![(0, 30_000)]
        );
    }

    #[test]
    fn incomplete_windows_are_recoverable_and_audio_tracks_are_isolated() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "media");
        storage
            .mark_window_running("media", 0, "v1", 0, 30_000, 1)
            .unwrap();
        storage
            .mark_window_failed("media", 0, "v1", 30_000, 60_000, 1, "interrupted")
            .unwrap();
        storage
            .mark_window_completed("media", 1, "v1", 0, 30_000, 1)
            .unwrap();

        assert!(
            storage
                .completed_windows("media", 0, "v1")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage.completed_windows("media", 1, "v1").unwrap(),
            vec![(0, 30_000)]
        );
    }

    #[test]
    fn provider_translations_do_not_overwrite_each_other() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "media");
        let segment = TranscriptSegment {
            id: "segment-1".into(),
            start_ms: 0,
            end_ms: 1_000,
            text: "Hello".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        };
        storage
            .store_transcript_segments("media", 0, "v1", std::slice::from_ref(&segment))
            .unwrap();
        for (provider, translated_text) in [("deepl", "Merhaba"), ("gemini", "Selam")] {
            storage
                .store_translations(
                    "media",
                    0,
                    "v1",
                    provider,
                    "TR",
                    &[SubtitleCue {
                        id: segment.id.clone(),
                        start_ms: segment.start_ms,
                        end_ms: segment.end_ms,
                        source_text: segment.text.clone(),
                        translated_text: Some(translated_text.into()),
                        source_language: Some("en".into()),
                        target_language: Some("TR".into()),
                        status: CueStatus::Ready,
                    }],
                )
                .unwrap();
        }

        let deepl = storage
            .load_translated_cues("media", 0, "v1", "deepl", "TR")
            .unwrap();
        let gemini = storage
            .load_translated_cues("media", 0, "v1", "gemini", "TR")
            .unwrap();
        assert_eq!(deepl[0].translated_text.as_deref(), Some("Merhaba"));
        assert_eq!(gemini[0].translated_text.as_deref(), Some("Selam"));
    }

    #[test]
    fn version_one_database_is_migrated_transactionally() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("legacy.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                 INSERT INTO schema_meta(key, value) VALUES('version', 1);
                 CREATE TABLE settings(key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at INTEGER NOT NULL);
                 CREATE TABLE media(
                   fingerprint TEXT PRIMARY KEY,
                   canonical_path TEXT NOT NULL,
                   size_bytes INTEGER NOT NULL,
                   modified_ms INTEGER NOT NULL,
                   duration_ms INTEGER NOT NULL,
                   playback_position_ms INTEGER NOT NULL DEFAULT 0,
                   last_opened_at INTEGER NOT NULL
                 );
                 CREATE TABLE processing_windows(
                   fingerprint TEXT NOT NULL, audio_track INTEGER NOT NULL,
                   pipeline_version TEXT NOT NULL, start_ms INTEGER NOT NULL,
                   end_ms INTEGER NOT NULL, generation INTEGER NOT NULL,
                   status TEXT NOT NULL, error TEXT, updated_at INTEGER NOT NULL,
                   PRIMARY KEY(fingerprint, audio_track, pipeline_version, start_ms, end_ms)
                 );
                 CREATE TABLE transcript_segments(
                   fingerprint TEXT NOT NULL, audio_track INTEGER NOT NULL,
                   pipeline_version TEXT NOT NULL, segment_id TEXT NOT NULL,
                   start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL,
                   detected_language TEXT, language_confidence REAL, is_final INTEGER NOT NULL,
                   PRIMARY KEY(fingerprint, audio_track, pipeline_version, segment_id)
                 );
                 CREATE TABLE translations(
                   fingerprint TEXT NOT NULL, audio_track INTEGER NOT NULL,
                   pipeline_version TEXT NOT NULL, provider_id TEXT NOT NULL,
                   target_language TEXT NOT NULL, segment_id TEXT NOT NULL,
                   translated_text TEXT, status TEXT NOT NULL,
                   PRIMARY KEY(fingerprint, audio_track, pipeline_version, provider_id, target_language, segment_id)
                 );",
            )
            .unwrap();
        drop(connection);

        let storage = Storage::open(&database).unwrap();
        let connection = storage.connection.lock().unwrap();
        let version: i64 = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        assert!(column_exists(&connection, "media", "cache_policy").unwrap());
    }

    #[test]
    fn newer_database_schema_is_rejected() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("future.sqlite3");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE schema_meta(key TEXT PRIMARY KEY, value INTEGER NOT NULL);
                 INSERT INTO schema_meta(key, value) VALUES('version', 999);",
            )
            .unwrap();
        drop(connection);
        assert!(matches!(
            Storage::open(&database),
            Err(StorageError::UnsupportedSchema { found: 999, .. })
        ));
    }

    #[test]
    fn cache_limit_evicts_oldest_media_but_preserves_active_media() {
        let (_directory, storage) = test_storage();
        for fingerprint in ["old", "active"] {
            insert_media(&storage, fingerprint);
            storage
                .store_transcript_segments(
                    fingerprint,
                    0,
                    "pipeline",
                    &[TranscriptSegment {
                        id: format!("{fingerprint}-segment"),
                        start_ms: 0,
                        end_ms: 1_000,
                        text: "x".repeat(2_000),
                        detected_language: None,
                        language_confidence: None,
                        is_final: true,
                    }],
                )
                .unwrap();
        }
        let removed = storage.enforce_cache_limit(1, Some("active")).unwrap();
        assert_eq!(removed, vec!["old"]);
        let connection = storage.connection.lock().unwrap();
        let active: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM media WHERE fingerprint = 'active'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(active, 1);
    }

    #[test]
    fn schema_migration_records_current_version_and_enables_wal() {
        let (_directory, storage) = test_storage();
        let connection = storage.connection.lock().unwrap();
        let version: i64 = connection
            .query_row(
                "SELECT value FROM schema_meta WHERE key = 'version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let journal_mode: String = connection
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, DATABASE_VERSION);
        assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    }

    #[test]
    fn replacing_a_window_removes_stale_segments_and_their_translations() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "media");
        let old = TranscriptSegment {
            id: "old".into(),
            start_ms: 100,
            end_ms: 900,
            text: "old text".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        };
        storage
            .store_transcript_segments("media", 0, "pipeline", std::slice::from_ref(&old))
            .unwrap();
        storage
            .store_translations(
                "media",
                0,
                "pipeline",
                "deepl",
                "TR",
                &[SubtitleCue {
                    id: old.id.clone(),
                    start_ms: old.start_ms,
                    end_ms: old.end_ms,
                    source_text: old.text.clone(),
                    translated_text: Some("eski".into()),
                    source_language: Some("en".into()),
                    target_language: Some("TR".into()),
                    status: CueStatus::Ready,
                }],
            )
            .unwrap();
        let replacement = TranscriptSegment {
            id: "new".into(),
            start_ms: 120,
            end_ms: 880,
            text: "new text".into(),
            detected_language: Some("en".into()),
            language_confidence: None,
            is_final: true,
        };
        storage
            .replace_window_segments(
                "media",
                0,
                "pipeline",
                0,
                1_000,
                1,
                std::slice::from_ref(&replacement),
            )
            .unwrap();
        assert_eq!(
            storage
                .load_transcript_segments("media", 0, "pipeline")
                .unwrap(),
            vec![replacement]
        );
        assert!(
            storage
                .load_translated_cues("media", 0, "pipeline", "deepl", "TR")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            storage.completed_windows("media", 0, "pipeline").unwrap(),
            vec![(0, 1_000)]
        );
    }

    #[test]
    fn purging_media_cache_cascades_through_all_generated_data() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "reset-me");
        let segment = TranscriptSegment {
            id: "segment-1".into(),
            start_ms: 100,
            end_ms: 900,
            text: "Reset this line".into(),
            detected_language: Some("en".into()),
            language_confidence: Some(0.99),
            is_final: true,
        };
        storage
            .replace_window_segments(
                "reset-me",
                0,
                "pipeline",
                0,
                1_000,
                1,
                std::slice::from_ref(&segment),
            )
            .unwrap();
        storage
            .store_translations(
                "reset-me",
                0,
                "pipeline",
                "deepl",
                "TR",
                &[SubtitleCue {
                    id: segment.id.clone(),
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                    source_text: segment.text.clone(),
                    translated_text: Some("Bu satırı sıfırla".into()),
                    source_language: Some("en".into()),
                    target_language: Some("TR".into()),
                    status: CueStatus::Ready,
                }],
            )
            .unwrap();
        storage.save_playback_position("reset-me", 42_000).unwrap();

        storage.purge_media_cache("reset-me").unwrap();

        let connection = storage.connection.lock().unwrap();
        for table in [
            "media",
            "processing_windows",
            "transcript_segments",
            "translations",
        ] {
            let count: i64 = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE fingerprint = 'reset-me'"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 0, "{table} retained reset data");
        }
    }

    #[test]
    fn ephemeral_media_is_removed_on_startup_cleanup() {
        let (_directory, storage) = test_storage();
        insert_media(&storage, "ephemeral");
        storage.set_media_cache_policy("ephemeral", false).unwrap();
        assert_eq!(storage.purge_ephemeral_cache().unwrap(), 1);
        assert_eq!(storage.playback_position("ephemeral").unwrap(), 0);
        let connection = storage.connection.lock().unwrap();
        let count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM media WHERE fingerprint = 'ephemeral'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn media_identity_changes_when_sample_changes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("video.bin");
        let mut file = File::create(&path).unwrap();
        file.write_all(b"video-one").unwrap();
        drop(file);
        let first = media_identity(&path).unwrap();

        let mut file = File::create(&path).unwrap();
        file.write_all(b"video-two").unwrap();
        drop(file);
        let second = media_identity(&path).unwrap();

        assert_ne!(first.fingerprint, second.fingerprint);
    }
}
