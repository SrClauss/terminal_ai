use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SESSION_TTL_SECONDS: u64 = 15 * 60; // 15 minutos

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub user_query: String,
    pub suggested_command: String,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub latency: Option<String>,
    pub execution_output: Option<String>,
    #[serde(default)]
    pub tokens_count: usize,
    pub timestamp: u64,
}

impl SessionEntry {
    /// Estima a contagem de tokens de um texto (aproximadamente 3.8 caracteres por token em média para código/bash/logs)
    pub fn estimate_tokens(text: &str) -> usize {
        let chars = text.chars().count();
        if chars == 0 {
            0
        } else {
            // Mínimo de 1 token se não estiver vazio, ~3.8 chars por token
            ((chars as f64) / 3.8).ceil() as usize
        }
    }

    /// Calcula os tokens totais deste step somando query, comando e saída
    pub fn compute_total_tokens(
        query: &str,
        command: &str,
        output: Option<&str>,
    ) -> usize {
        let q_tokens = Self::estimate_tokens(query);
        let cmd_tokens = Self::estimate_tokens(command);
        let out_tokens = output.map(Self::estimate_tokens).unwrap_or(0);
        q_tokens + cmd_tokens + out_tokens
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub history: Vec<SessionEntry>,
}

impl SessionState {
    fn session_path() -> Option<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "terminal_ai", "tai")?;
        let cache_dir = proj_dirs.cache_dir();
        let _ = create_dir_all(cache_dir);
        Some(cache_dir.join("session.json"))
    }

    pub fn steps_log_path() -> Option<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "terminal_ai", "tai")?;
        let cache_dir = proj_dirs.cache_dir();
        let _ = create_dir_all(cache_dir);
        Some(cache_dir.join("steps.log"))
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn decompose_steps(command: &str) -> Vec<String> {
        if command.contains(" && ") {
            command
                .split(" && ")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            vec![command.trim().to_string()]
        }
    }

    pub fn load_active() -> Self {
        let path = match Self::session_path() {
            Some(p) => p,
            None => return Self::default(),
        };

        if !path.exists() {
            return Self::default();
        }

        let mut file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return Self::default(),
        };

        let mut content = String::new();
        if file.read_to_string(&mut content).is_err() {
            return Self::default();
        }

        let session: SessionState = match serde_json::from_str(&content) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };

        // Verifica se a última interação expirou por TTL
        let now = Self::current_timestamp();
        if let Some(last) = session.history.last() {
            if now.saturating_sub(last.timestamp) > SESSION_TTL_SECONDS {
                // Sessão expirada
                let _ = std::fs::remove_file(&path);
                return Self::default();
            }
        }

        session
    }

    /// Retorna o total de tokens atualmente consumidos no histórico
    pub fn total_tokens(&self) -> usize {
        self.history.iter().map(|e| e.tokens_count).sum()
    }

    /// Poda o histórico mais antigo (FIFO) para respeitar o limite máximo de tokens
    pub fn prune_to_token_budget(&mut self, max_tokens: usize) {
        while self.total_tokens() > max_tokens && !self.history.is_empty() {
            self.history.remove(0);
        }
    }

    pub fn record(&mut self, query: &str, command: &str) {
        self.record_full(query, command, None, None, None);
    }

    pub fn record_with_output(&mut self, query: &str, command: &str, output: Option<String>) {
        self.record_full(query, command, None, output, None);
    }

    pub fn record_full(
        &mut self,
        query: &str,
        command: &str,
        latency: Option<&str>,
        output: Option<String>,
        exit_code: Option<i32>,
    ) {
        let max_budget = crate::config::Config::load()
            .map(|c| c.max_context_tokens)
            .unwrap_or(50000);

        let steps = Self::decompose_steps(command);
        let tokens_count = SessionEntry::compute_total_tokens(
            query,
            command,
            output.as_deref(),
        );

        let entry = SessionEntry {
            user_query: query.to_string(),
            suggested_command: command.to_string(),
            steps: steps.clone(),
            latency: latency.map(|s| s.to_string()),
            execution_output: output,
            tokens_count,
            timestamp: Self::current_timestamp(),
        };
        self.history.push(entry);

        // Poda retroativa garantindo que o total de tokens caiba no orçamento configurado
        self.prune_to_token_budget(max_budget);

        self.save();
        Self::append_steps_log(query, command, &steps, latency, exit_code, tokens_count);
    }

    pub fn append_steps_log(
        query: &str,
        command: &str,
        steps: &[String],
        latency: Option<&str>,
        exit_code: Option<i32>,
        tokens_count: usize,
    ) {
        if let Some(path) = Self::steps_log_path() {
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let ts = Self::current_timestamp();
                let latency_tag = latency.map(|l| format!(" [{l}]")).unwrap_or_default();
                let exit_tag = exit_code
                    .map(|code| format!(" (exit code: {code})"))
                    .unwrap_or_else(|| " (sugerido)".to_string());
                let tokens_tag = format!(" [tokens: {tokens_count}]");

                let _ = writeln!(file, "--- [epoch:{ts}]{latency_tag}{tokens_tag}{exit_tag} ---");
                let _ = writeln!(file, "Prompt:  {query}");
                let _ = writeln!(file, "Comando: {command}");
                if steps.len() > 1 {
                    for (i, st) in steps.iter().enumerate() {
                        let _ = writeln!(file, "  Passo {}: {}", i + 1, st);
                    }
                }
                let _ = writeln!(file);
            }
        }
    }

    pub fn print_recent_logs(limit: usize) {
        use colored::Colorize;

        let session = Self::load_active();
        if session.history.is_empty() {
            println!("{}", "Nenhum passo registrado no histórico recente.".yellow());
            return;
        }

        let max_budget = crate::config::Config::load()
            .map(|c| c.max_context_tokens)
            .unwrap_or(50000);
        let total_active_tokens = session.total_tokens();

        println!("{}", "════════════════════════════════════════════════════════════".cyan());
        println!("{}", " HISTÓRICO DE PASSOS E EXECUÇÕES RECENTES (TAI LOG)".bold().cyan());
        println!("{}\n", "════════════════════════════════════════════════════════════".cyan());

        let start_idx = session.history.len().saturating_sub(limit);
        for (idx, entry) in session.history[start_idx..].iter().enumerate() {
            let num = start_idx + idx + 1;
            let latency_badge = entry
                .latency
                .as_deref()
                .map(|l| format!(" [⏱ {l}]"))
                .unwrap_or_default();
            let tokens_badge = format!(" [🪙 {} tokens]", entry.tokens_count);

            println!(
                "{} {}{}{}",
                format!("#{num}").bold().yellow(),
                entry.user_query.bold().white(),
                latency_badge.dimmed(),
                tokens_badge.cyan().dimmed()
            );
            println!("   {} {}", "Comando:".cyan(), entry.suggested_command.green());

            if entry.steps.len() > 1 {
                println!("   {}", "Passos decompostos:".blue());
                for (s_idx, step) in entry.steps.iter().enumerate() {
                    println!("     {}. {}", (s_idx + 1).to_string().cyan(), step.green());
                }
            }

            if let Some(ref out) = entry.execution_output {
                let first_line = out.lines().next().unwrap_or("").trim();
                if !first_line.is_empty() {
                    println!("   {} {}", "Saída:".dimmed(), first_line.dimmed());
                }
            }
            println!("{}", "────────────────────────────────────────────────────────────".dimmed());
        }

        let pct = if max_budget > 0 {
            (total_active_tokens as f64 / max_budget as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "{}",
            format!(
                "📊 Janela de Contexto Ativa: {} / {} tokens ({:.1}% em uso)",
                total_active_tokens, max_budget, pct
            )
            .bold()
            .blue()
        );

        if let Some(log_path) = Self::steps_log_path() {
            println!("{}", format!("Log persistente completo gravado em: {}", log_path.display()).dimmed());
        }
    }

    pub fn save(&self) {
        if let Some(path) = Self::session_path() {
            if let Ok(serialized) = serde_json::to_string_pretty(self) {
                if let Ok(mut file) = File::create(path) {
                    let _ = file.write_all(serialized.as_bytes());
                }
            }
        }
    }

    pub fn clear() {
        if let Some(path) = Self::session_path() {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn format_for_prompt(&self) -> String {
        if self.history.is_empty() {
            return String::new();
        }

        let max_budget = crate::config::Config::load()
            .map(|c| c.max_context_tokens)
            .unwrap_or(50000);
        // Reserva até 70% do orçamento para as saídas de terminal do histórico
        let max_output_chars = (max_budget * 4).saturating_mul(7) / 10;

        let mut out = String::from("\nRECENT INTERACTION HISTORY & REAL EXECUTION OUTPUTS (Crucial: use the command outputs to inspect files/results and decide the next steps):\n");
        for (i, entry) in self.history.iter().enumerate() {
            out.push_str(&format!(
                "Turn {}:\n- User requested: '{}'\n- Command executed: '{}'\n",
                i + 1,
                entry.user_query,
                entry.suggested_command
            ));
            if let Some(ref out_text) = entry.execution_output {
                let trimmed = out_text.trim();
                let output_snippet = if trimmed.len() > max_output_chars {
                    format!("{}... [truncated]", &trimmed[..max_output_chars])
                } else {
                    trimmed.to_string()
                };
                out.push_str(&format!("- Terminal Output / Result: '{}'\n", output_snippet));
            }
        }
        out
    }
}

impl SessionState {
    /// Detecta rapidamente (em menos de 2ms) a stack de desenvolvimento do diretório atual
    pub fn detect_local_stack() -> Option<String> {
        use std::path::Path;

        let mut tags = Vec::new();

        if Path::new("Cargo.toml").exists() {
            tags.push("Rust (Cargo)");
        }
        if Path::new("package.json").exists() {
            if Path::new("tsconfig.json").exists() {
                tags.push("TypeScript / Node.js");
            } else {
                tags.push("JavaScript / Node.js");
            }
        }
        if Path::new("pyproject.toml").exists() || Path::new("requirements.txt").exists() || Path::new("Pipfile").exists() {
            tags.push("Python");
        }
        if Path::new("go.mod").exists() {
            tags.push("Go");
        }
        if Path::new("pom.xml").exists() || Path::new("build.gradle").exists() {
            tags.push("Java / Kotlin");
        }
        if Path::new("docker-compose.yml").exists() || Path::new("compose.yaml").exists() || Path::new("Dockerfile").exists() {
            tags.push("Docker");
        }
        if Path::new(".git").exists() {
            tags.push("Git Repository");
        }

        if tags.is_empty() {
            None
        } else {
            Some(tags.join(", "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_and_format_prompt() {
        let mut session = SessionState::default();
        session.record("listar arquivos de log", "find . -name '*.log'");
        let prompt_context = session.format_for_prompt();
        assert!(prompt_context.contains("find . -name '*.log'"));
        assert!(prompt_context.contains("listar arquivos de log"));
    }

    #[test]
    fn test_detect_local_stack_in_repo() {
        let stack = SessionState::detect_local_stack();
        assert!(stack.is_some());
        let s = stack.unwrap();
        assert!(s.contains("Rust (Cargo)"));
        assert!(s.contains("Git Repository"));
    }

    #[test]
    fn test_decompose_steps() {
        let single = SessionState::decompose_steps("cargo check");
        assert_eq!(single, vec!["cargo check"]);

        let chained = SessionState::decompose_steps("git fetch && git merge && cargo build");
        assert_eq!(chained, vec!["git fetch", "git merge", "cargo build"]);
    }

    #[test]
    fn test_record_full_with_steps_and_latency() {
        let mut session = SessionState::default();
        session.record_full(
            "atualizar e testar",
            "git pull && cargo test",
            Some("1.25s"),
            Some("test result: ok".to_string()),
            Some(0),
        );
        assert_eq!(session.history.len(), 1);
        let entry = &session.history[0];
        assert_eq!(entry.user_query, "atualizar e testar");
        assert_eq!(entry.suggested_command, "git pull && cargo test");
        assert_eq!(entry.steps, vec!["git pull", "cargo test"]);
        assert_eq!(entry.latency.as_deref(), Some("1.25s"));
        assert_eq!(entry.execution_output.as_deref(), Some("test result: ok"));
        assert!(entry.tokens_count > 0);
    }

    #[test]
    fn test_token_estimation_and_pruning() {
        assert_eq!(SessionEntry::estimate_tokens(""), 0);
        let sample = "echo hello world";
        let tokens = SessionEntry::estimate_tokens(sample);
        assert!(tokens >= 4);

        let mut session = SessionState::default();
        let query = "listar";
        let cmd = "ls -la";
        let output = "total 64\n-rw-r--r-- 1 claus claus 1024 out 12 file.txt";
        let entry_tokens = SessionEntry::compute_total_tokens(query, cmd, Some(output));

        for _ in 0..5 {
            session.history.push(SessionEntry {
                user_query: query.to_string(),
                suggested_command: cmd.to_string(),
                steps: vec![cmd.to_string()],
                latency: None,
                execution_output: Some(output.to_string()),
                tokens_count: entry_tokens,
                timestamp: SessionState::current_timestamp(),
            });
        }
        assert_eq!(session.history.len(), 5);

        // Se o orçamento permitir apenas 2 entradas completas:
        let max_budget = entry_tokens * 2 + 5;
        session.prune_to_token_budget(max_budget);
        assert_eq!(session.history.len(), 2);
    }
}
