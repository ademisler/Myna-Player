# Security exceptions

Myna Player does not ignore known exploitable vulnerabilities. The following
RustSec entries are temporary maintenance-status exceptions in the Leptos 0.7
macro dependency graph. They do not describe a known memory-safety or remote-code
execution defect in Myna Player.

| Advisory | Transitive crate | Reason | Review deadline |
| --- | --- | --- | --- |
| RUSTSEC-2024-0436 | `paste` | Pulled by Leptos/Tachys 0.7; upstream marks it unmaintained and provides no compatible safe upgrade. | 2026-08-31 |
| RUSTSEC-2026-0173 | `proc-macro-error2` | Pulled by Leptos macros 0.7; upstream recommends migration but the compatible Leptos line has not removed it. | 2026-08-31 |

The exceptions must be removed when the frontend is upgraded to a Leptos release
that no longer depends on these crates. CI still denies all other advisories,
yanked dependencies, unknown registries, unknown Git sources, wildcard version
requirements, and unapproved licenses.
