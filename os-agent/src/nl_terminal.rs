use crate::ai_client::{AiClient, Message};
use crate::app_generator::{generate_app_spec, AppGenerator};
use crate::intent::{classify, IntentKind};
use anyhow::Result;
use rustyline::DefaultEditor;

pub struct NlTerminal {
    client: AiClient,
}

impl NlTerminal {
    pub fn new(client: AiClient) -> Self {
        Self { client }
    }

    pub async fn run(&mut self) -> Result<()> {
        println!("AIOS v0.1.0 — The OS that assumes you have AI.");
        println!("Type your intent in natural language. Type 'exit' to quit.\n");

        // Detect if stdin is a TTY; if not, fall back to plain BufRead
        let is_tty = unsafe { libc::isatty(libc::STDIN_FILENO) } != 0;

        if is_tty {
            self.run_interactive().await
        } else {
            self.run_piped().await
        }
    }

    async fn run_interactive(&mut self) -> Result<()> {
        let mut rl = DefaultEditor::new()?;
        loop {
            let readline = rl.readline("aios> ");
            match readline {
                Ok(line) => {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    rl.add_history_entry(&line).ok();
                    if !self.handle_input(&line).await {
                        break;
                    }
                }
                Err(rustyline::error::ReadlineError::Interrupted) => {
                    println!("Interrupted. Use 'shutdown' to exit.");
                }
                Err(rustyline::error::ReadlineError::Eof) => {
                    println!("\nGoodbye.");
                    break;
                }
                Err(e) => {
                    eprintln!("Error: {}", e);
                    break;
                }
            }
        }
        Ok(())
    }

    async fn run_piped(&mut self) -> Result<()> {
        use std::io::BufRead;
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let line = line?.trim().to_string();
            if line.is_empty() {
                continue;
            }
            println!("aios> {}", line);
            if !self.handle_input(&line).await {
                break;
            }
        }
        println!("\nGoodbye.");
        Ok(())
    }

    async fn handle_input(&self, input: &str) -> bool {
        if input.eq_ignore_ascii_case("shutdown") || input.eq_ignore_ascii_case("exit") {
            println!("Shutting down AIOS...");
            return false;
        }

        print!("  Classifying intent... ");
        use std::io::Write;
        std::io::stdout().flush().ok();

        match classify(input, &self.client).await {
            Ok(intent) => {
                println!("{:?}", intent.kind);
                match intent.kind {
                    IntentKind::CreateApp => {
                        self.handle_create_app(input).await;
                    }
                    IntentKind::FileOperation => {
                        self.handle_file_op(input).await;
                    }
                    IntentKind::SystemQuery => {
                        self.handle_system_query(input).await;
                    }
                    IntentKind::RunApp => {
                        println!("  → Run app: {}", intent.description);
                        println!("    (App runner not yet wired up)");
                    }
                    IntentKind::InstallApp => {
                        println!("  → Install from store: {}", intent.description);
                        println!("    (Store client not yet wired up)");
                    }
                    IntentKind::Unknown => {
                        println!("  → Could not understand intent. Please try rephrasing.");
                    }
                }
            }
            Err(e) => {
                eprintln!("  Error classifying intent: {}", e);
            }
        }
        true
    }

    async fn handle_create_app(&self, prompt: &str) {
        println!("  → Generating AppSpec from prompt...");
        let spec = match generate_app_spec(prompt, &self.client).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  Failed to generate app spec: {}", e);
                return;
            }
        };
        println!("  → App: \"{}\" — {}", spec.name, spec.description);
        println!("  → Generating Rust/Wasm code (this may take ~30s)...");

        let generator = AppGenerator::new(self.client.clone());
        match generator.generate_and_install(prompt, &spec).await {
            Ok(app) => {
                println!("  ✓ App installed!");
                println!("    UUID: {}", app.app_uuid);
                println!("    Package: {}", app.osp_path.display());
            }
            Err(e) => {
                eprintln!("  ✗ App generation failed: {}", e);
            }
        }
    }

    async fn handle_file_op(&self, input: &str) {
        // Ask AI to execute the file operation as a shell command
        let messages = vec![
            Message::system("You are the AIOS shell. The user wants a file system operation. \
                Respond with ONLY the output of the operation — run it mentally and output what `bash -c` would print. \
                No explanations, just the output. If you need to execute something that modifies the filesystem, \
                say: [WOULD EXECUTE: <command>] instead."),
            Message::user(input.to_string()),
        ];
        match self.client.complete(messages).await {
            Ok(response) => println!("  {}", response.trim()),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }

    async fn handle_system_query(&self, input: &str) {
        // Collect real system info and let AI summarize
        let mem_info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let cpu_info = std::fs::read_to_string("/proc/cpuinfo")
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("model name") || l.starts_with("cpu MHz"))
            .take(4)
            .collect::<Vec<_>>()
            .join("\n");
        let load_avg = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();

        let context = format!(
            "System info:\n/proc/meminfo (first 10 lines):\n{}\n\nCPU:\n{}\n\nLoad avg: {}",
            mem_info.lines().take(10).collect::<Vec<_>>().join("\n"),
            cpu_info,
            load_avg.trim()
        );

        let messages = vec![
            Message::system(
                "You are AIOS system monitor. Answer the user's system query concisely \
                based on the provided /proc data. Use Chinese if the question is in Chinese. \
                Format numbers in human-readable form (MB/GB).",
            ),
            Message::user(format!("{}\n\nQuery: {}", context, input)),
        ];
        match self.client.complete(messages).await {
            Ok(response) => println!("  {}", response.trim()),
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
}
