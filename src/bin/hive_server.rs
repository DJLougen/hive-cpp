//! `hive-server` — placeholder for the future in-process LLM server.
//!
//! Step 2 keeps the binary as a stub. Step 3 will wire it up against
//! the `llm` module (OpenAI-compat HTTP) and the `busybee` policy so
//! the binary can serve a real agent loop end-to-end without a
//! separate vLLM / llama.cpp process.

use std::process::ExitCode;

fn main() -> ExitCode {
    eprintln!("hive-server: not implemented in Step 2");
    ExitCode::from(1)
}
