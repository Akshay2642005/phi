//! Tool skills — skills that interact with real external systems.
//!
//! These are the "hands" of the digital worker. Unlike analysis skills
//! that only call the LLM, tool skills reach out to the world:
//! web search, HTTP calls, file I/O, code execution, etc.
//!
//! All tool skills implement `Skill<WorkerInput>` so they can be
//! registered in the same registry as analysis skills.

pub mod web_search;
