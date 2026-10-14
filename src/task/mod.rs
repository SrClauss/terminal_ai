use crate::executor::CommandExecutor;
use crate::providers::OpenAiCompatibleProvider;
use crate::safety::{DangerLevel, SafetyAnalyzer};
use crate::session::SessionState;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Continue,
    Done,
    Blocked,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TaskStatus::Continue => write!(f, "CONTINUE"),
            TaskStatus::Done => write!(f, "DONE"),
            TaskStatus::Blocked => write!(f, "BLOCKED"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDecision {
    pub status: TaskStatus,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub thought: String,
    #[serde(default)]
    pub explanation: String,
    #[serde(default)]
    pub affects_parent_shell: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStepRecord {
    pub step_number: usize,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub latency_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DifficultyLevel {
    Low,
    Medium,
    High,
}

impl fmt::Display for DifficultyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DifficultyLevel::Low => write!(f, "Baixa"),
            DifficultyLevel::Medium => write!(f, "Média"),
            DifficultyLevel::High => write!(f, "Alta"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDiagnostic {
    pub difficulty: DifficultyLevel,
    pub reason: String,
    pub failed_steps_count: usize,
    pub summary_of_errors: String,
    pub suggested_next_direction: String,
}

impl TaskDiagnostic {
    /// Compila deterministicamente os erros ocorridos no histórico da tarefa e avalia a dificuldade
    pub fn deterministic_assess(goal: &str, history: &[TaskStepRecord]) -> Self {
        let failed_steps: Vec<&TaskStepRecord> = history
            .iter()
            .filter(|s| s.exit_code != 0 || !s.stderr.trim().is_empty())
            .collect();

        let failed_steps_count = failed_steps.len();

        let mut error_lines: Vec<String> = Vec::new();
        for step in &failed_steps {
            let src = if !step.stderr.trim().is_empty() {
                &step.stderr
            } else {
                &step.stdout
            };

            for line in src.lines().take(5) {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !error_lines.iter().any(|e| e == trimmed) {
                    error_lines.push(format!("Passo {} (`{}`): {}", step.step_number, step.command, trimmed));
                }
            }
        }

        let summary_of_errors = if error_lines.is_empty() {
            "Nenhum erro explícito reportado; os comandos executaram mas a meta ainda necessita de passos adicionais.".to_string()
        } else {
            error_lines.join("\n")
        };

        let (difficulty, reason, suggested_next_direction) = if failed_steps_count == 0 {
            (
                DifficultyLevel::Low,
                format!("A tarefa '{goal}' está progredindo normalmente sem falhas de comando, porém é composta por múltiplas etapas que excederam o orçamento inicial de passos."),
                "Conceder mais passos para a IA prosseguir com a sequência natural de execução.".to_string(),
            )
        } else if failed_steps_count <= 2 {
            (
                DifficultyLevel::Medium,
                format!("Ocorreram {failed_steps_count} falha(s) pontuais durante a execução (ex: argumentos, arquivos ausentes ou dependências intermediárias)."),
                "Permitir que a IA aplique comandos de contorno ou crie os pré-requisitos identificados nas saídas de erro.".to_string(),
            )
        } else {
            (
                DifficultyLevel::High,
                format!("Identificadas {failed_steps_count} falhas consecutivas ou persistentes. O objetivo apresenta complexidade elevada ou barreiras de ambiente/permissão."),
                "Verificar permissões de acesso (sudo/root), dependências do sistema operacional ou ajustar a formulação da tarefa.".to_string(),
            )
        };

        Self {
            difficulty,
            reason,
            failed_steps_count,
            summary_of_errors,
            suggested_next_direction,
        }
    }

    /// Avalia o diagnóstico utilizando a LLM quando disponível, caindo de volta para a compilação determinística
    pub async fn assess(
        goal: &str,
        history: &[TaskStepRecord],
        provider: Option<&OpenAiCompatibleProvider>,
        language: &str,
    ) -> Self {
        let base = Self::deterministic_assess(goal, history);

        let provider = match provider {
            Some(p) => p,
            None => return base,
        };

        let system_prompt = format!(
            r#"You are an AI diagnosing an autonomous terminal agent loop.
Language: {language}.
The agent had a limited step budget and did not complete the goal yet.
Analyze the executed steps and errors, assess task difficulty, and output strictly JSON:
{{
  "difficulty": "low" | "medium" | "high",
  "reason": "Explication in language '{language}' why this task is low/medium/high difficulty",
  "summary_of_errors": "Concise summary of what failed in language '{language}'",
  "suggested_next_direction": "What the agent should attempt next in language '{language}'"
}}"#
        );

        let mut user_prompt = format!("GOAL: {goal}\n\nEXECUTION HISTORY:\n");
        for step in history {
            user_prompt.push_str(&format!(
                "Step {}: `{}` (Exit Code: {})\nStdout:\n{}\nStderr:\n{}\n\n",
                step.step_number, step.command, step.exit_code, step.stdout.trim(), step.stderr.trim()
            ));
        }

        if let Ok(raw_response) = provider.send_raw_prompt(&system_prompt, &user_prompt).await {
            if let Some(parsed) = Self::parse_ai_diagnostic(&raw_response, base.failed_steps_count) {
                return parsed;
            }
        }

        base
    }

    fn parse_ai_diagnostic(raw: &str, failed_count: usize) -> Option<Self> {
        #[derive(Deserialize)]
        struct DiagnosticPayload {
            difficulty: String,
            reason: String,
            summary_of_errors: String,
            suggested_next_direction: String,
        }

        let clean = extract_json_block(raw);
        let payload: DiagnosticPayload = serde_json::from_str(&clean).ok()?;

        let difficulty = match payload.difficulty.to_lowercase().as_str() {
            "low" | "baixa" => DifficultyLevel::Low,
            "high" | "alta" => DifficultyLevel::High,
            _ => DifficultyLevel::Medium,
        };

        Some(Self {
            difficulty,
            reason: payload.reason,
            failed_steps_count: failed_count,
            summary_of_errors: payload.summary_of_errors,
            suggested_next_direction: payload.suggested_next_direction,
        })
    }
}

pub fn extract_json_block(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    } else if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }

    if let (Some(s), Some(e)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if s <= e {
            return trimmed[s..=e].to_string();
        }
    }

    trimmed.to_string()
}

pub fn parse_task_decision(raw: &str) -> Result<TaskDecision, String> {
    let json_text = extract_json_block(raw);

    if let Ok(decision) = serde_json::from_str::<TaskDecision>(&json_text) {
        return Ok(decision);
    }

    // Fallback: se o modelo respondeu com marcador %%%
    if let Some(start) = raw.find("%%%") {
        let after = &raw[start + 3..];
        if let Some(end) = after.find("%%%") {
            let cmd = after[..end].trim().to_string();
            return Ok(TaskDecision {
                status: TaskStatus::Continue,
                command: Some(cmd),
                thought: "Comando extraído via fallback de delimitador.".to_string(),
                explanation: after[end + 3..].trim().to_string(),
                affects_parent_shell: false,
            });
        }
    }

    // Fallback: detecção de conclusão textual
    let lower = raw.to_lowercase();
    if lower.contains("\"status\": \"done\"") || lower.contains("\"done\"") || lower.contains("tarefa concluída") || lower.contains("objetivo atingido") {
        return Ok(TaskDecision {
            status: TaskStatus::Done,
            command: None,
            thought: "Conclusão detectada no texto da resposta.".to_string(),
            explanation: raw.trim().to_string(),
            affects_parent_shell: false,
        });
    }

    Err(format!("Não foi possível interpretar a decisão da IA: {raw}"))
}

pub fn build_step_system_prompt(os_info: &str, language: &str, rules: Option<&str>) -> String {
    let rules_section = match rules {
        Some(r) if !r.trim().is_empty() => format!("\nUSER CUSTOM RULES:\n{}\n", r.trim()),
        _ => String::new(),
    };

    format!(
        r#"You are Terminal AI's Autonomous Agent Loop ('tai task').
Environment: {os_info}.
Language for explanations: {language}.{rules_section}

You receive a high-level user GOAL and the full EXECUTION HISTORY of terminal commands executed so far (with real stdout, stderr, and exit codes).
You iteratively execute actions until the goal is 100% finished.

MANDATORY RESPONSE FORMAT:
You MUST respond with a single valid JSON block:
```json
{{
  "status": "continue" | "done" | "blocked",
  "thought": "Your reasoning on what has already been achieved and what must be done next",
  "command": "The single bash command line to run next (leave empty or null if done/blocked)",
  "explanation": "Didactic and brief explanation in language '{language}'",
  "affects_parent_shell": false
}}
```

OPERATIONAL RULES:
1. Status values:
   - "continue": The goal is not yet finished. Provide the exact next bash command.
   - "done": The goal has been completely achieved. Set "command" to "" and explain the result.
   - "blocked": The goal cannot proceed automatically (needs human password/sudo, captcha, or manual intervention).
2. Look at execution history and stdout/stderr:
   - If previous command failed, do not repeat it verbatim. Diagnose and fix the root cause.
   - If files or services need inspection, run check commands (e.g. `ls`, `grep`, `systemctl status`, `curl`).
3. You can use persistent python venv at `~/.local/share/tai/venv/bin/python` if scripts or calculations are needed.
4. Output STRICTLY the JSON block. Do not add casual chat."#
    )
}

pub fn build_step_user_prompt(goal: &str, history: &[TaskStepRecord]) -> String {
    let mut s = format!("GOAL: {goal}\n\n");
    if history.is_empty() {
        s.push_str("No steps executed yet. What is the first command to start resolving this goal?");
    } else {
        s.push_str(&format!("EXECUTION HISTORY ({} step(s) completed so far):\n", history.len()));
        for step in history {
            s.push_str(&format!(
                "--- Step {} ---\nCommand: `{}`\nExit Code: {}\nStdout:\n{}\nStderr:\n{}\n\n",
                step.step_number,
                step.command,
                step.exit_code,
                step.stdout.trim(),
                step.stderr.trim()
            ));
        }
        s.push_str("Based on the real terminal outputs above, what is the next step?");
    }
    s
}

#[derive(Debug)]
pub enum TaskRunResult {
    Completed {
        explanation: String,
        total_steps: usize,
        history: Vec<TaskStepRecord>,
    },
    MaxStepsReached {
        diagnostic: TaskDiagnostic,
        total_steps: usize,
        history: Vec<TaskStepRecord>,
    },
    Blocked {
        reason: String,
        total_steps: usize,
        history: Vec<TaskStepRecord>,
    },
    SafetyAbort {
        command: String,
        danger_reason: String,
    },
}

pub type TaskCallback = Box<dyn FnMut(&TaskDiagnostic) -> bool + Send>;

pub struct TaskRunner {
    pub goal: String,
    pub max_steps: usize,
    pub current_step: usize,
    pub history: Vec<TaskStepRecord>,
    pub extension_callback: Option<TaskCallback>,
}

impl TaskRunner {
    pub fn new(goal: impl Into<String>, max_steps: usize) -> Self {
        Self {
            goal: goal.into(),
            max_steps,
            current_step: 0,
            history: Vec::new(),
            extension_callback: None,
        }
    }

    pub fn with_extension_callback<F>(mut self, cb: F) -> Self
    where
        F: FnMut(&TaskDiagnostic) -> bool + Send + 'static,
    {
        self.extension_callback = Some(Box::new(cb));
        self
    }

    /// Exibe no terminal o painel com diagnóstico compilado e pergunta ao usuário se aceita +5 passos
    pub fn prompt_user_extension(diag: &TaskDiagnostic) -> Result<bool, Box<dyn std::error::Error>> {
        use inquire::Select;

        println!("\n{}", "════════════════════════════════════════════════════════════".yellow());
        println!("{}", " 🛑 CHECKPOINT DE PASSOS: LIMITE ATINGIDO".bold().yellow());
        println!("{}", "════════════════════════════════════════════════════════════".yellow());

        let diff_badge = match diag.difficulty {
            DifficultyLevel::Low => diag.difficulty.to_string().green().bold(),
            DifficultyLevel::Medium => diag.difficulty.to_string().yellow().bold(),
            DifficultyLevel::High => diag.difficulty.to_string().red().bold(),
        };

        println!("  {} {}", "Dificuldade Avaliada:".bold().white(), diff_badge);
        println!("  {} {}", "Diagnóstico:".bold().white(), diag.reason.cyan());

        if diag.failed_steps_count > 0 {
            println!("\n  {}", "Erros Compilados nas Execuções:".bold().red());
            for line in diag.summary_of_errors.lines().take(8) {
                println!("    • {}", line.dimmed());
            }
        }

        println!("\n  {} {}", "Direção Sugerida:".bold().white(), diag.suggested_next_direction.green());
        println!("{}", "────────────────────────────────────────────────────────────".dimmed());

        let options = vec![
            "Sim, conceder mais 5 passos para a IA continuar",
            "Não, encerrar a execução da tarefa agora",
        ];

        let choice = Select::new("Deseja conceder mais 5 passos para a IA?", options).prompt()?;
        Ok(choice.starts_with("Sim"))
    }

    /// Executa o loop da tarefa
    pub async fn run(
        &mut self,
        provider: &OpenAiCompatibleProvider,
        os_info: &str,
        language: &str,
        custom_rules: Option<&str>,
        auto_mode: bool,
    ) -> Result<TaskRunResult, Box<dyn std::error::Error>> {
        let system_prompt = build_step_system_prompt(os_info, language, custom_rules);

        loop {
            // Verifica se atingiu o limite de passos
            if self.current_step >= self.max_steps {
                let diagnostic = TaskDiagnostic::assess(&self.goal, &self.history, Some(provider), language).await;

                let should_continue = if let Some(ref mut cb) = self.extension_callback {
                    cb(&diagnostic)
                } else {
                    Self::prompt_user_extension(&diagnostic)?
                };

                if should_continue {
                    println!("{}", "\n✓ Mais 5 passos concedidos! Retomando ciclo da tarefa...".green().bold());
                    self.max_steps += 5;
                } else {
                    println!("{}", "\nLimite de passos mantido. Encerrando ciclo da tarefa.".yellow());
                    return Ok(TaskRunResult::MaxStepsReached {
                        diagnostic,
                        total_steps: self.history.len(),
                        history: self.history.clone(),
                    });
                }
            }

            self.current_step += 1;
            let step_idx = self.current_step;

            println!(
                "\n{}",
                format!("▶ [Passo {}/{}] Consultando IA sobre próximo movimento...", step_idx, self.max_steps)
                    .bold()
                    .cyan()
            );

            let user_prompt = build_step_user_prompt(&self.goal, &self.history);
            let raw_response = provider.send_raw_prompt(&system_prompt, &user_prompt).await?;
            let decision = parse_task_decision(&raw_response).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

            if !decision.thought.trim().is_empty() {
                println!("  {} {}", "Raciocínio:".dimmed(), decision.thought.dimmed());
            }

            match decision.status {
                TaskStatus::Done => {
                    println!("\n{}", "════════════════════════════════════════════════════════════".green());
                    println!("{}", " 🎉 OBJETIVO CUMPRIDO COM SUCESSO!".bold().green());
                    println!("{}", "════════════════════════════════════════════════════════════".green());
                    if !decision.explanation.is_empty() {
                        println!("  {}\n", decision.explanation.white());
                    }
                    return Ok(TaskRunResult::Completed {
                        explanation: decision.explanation,
                        total_steps: self.history.len(),
                        history: self.history.clone(),
                    });
                }
                TaskStatus::Blocked => {
                    println!("\n{}", "════════════════════════════════════════════════════════════".red());
                    println!("{}", " ⚠️ TAREFA BLOQUEADA: INTERVENÇÃO MANUAL NECESSÁRIA".bold().red());
                    println!("{}", "════════════════════════════════════════════════════════════".red());
                    println!("  {}\n", decision.explanation.yellow());
                    return Ok(TaskRunResult::Blocked {
                        reason: decision.explanation,
                        total_steps: self.history.len(),
                        history: self.history.clone(),
                    });
                }
                TaskStatus::Continue => {
                    let cmd = match decision.command {
                        Some(ref c) if !c.trim().is_empty() => c.trim(),
                        _ => {
                            // Se status é continue mas comando veio vazio, trata como conclusão
                            return Ok(TaskRunResult::Completed {
                                explanation: decision.explanation,
                                total_steps: self.history.len(),
                                history: self.history.clone(),
                            });
                        }
                    };

                    println!("  {} {}", "Comando:".bold().white(), cmd.bold().green());
                    if !decision.explanation.is_empty() {
                        println!("  {} {}", "Ação:".cyan(), decision.explanation.dimmed());
                    }

                    // Checagem de segurança
                    let safety = SafetyAnalyzer::assess(cmd);
                    if safety.level == DangerLevel::Catastrophic {
                        SafetyAnalyzer::print_warning(&safety);
                        return Ok(TaskRunResult::SafetyAbort {
                            command: cmd.to_string(),
                            danger_reason: safety.reason.unwrap_or_else(|| "Comando catastrófico bloqueado".to_string()),
                        });
                    }

                    // Se não for modo auto (-y), pede confirmação para o passo
                    if !auto_mode {
                        use inquire::Select;
                        let step_choices = vec!["Executar passo", "Pular passo", "Cancelar tarefa"];
                        let choice = Select::new("Ação para este passo:", step_choices).prompt()?;
                        match choice {
                            "Executar passo" => {}
                            "Pular passo" => continue,
                            _ => {
                                println!("{}", "Tarefa cancelada pelo usuário.".dimmed());
                                return Ok(TaskRunResult::Blocked {
                                    reason: "Cancelado pelo usuário".to_string(),
                                    total_steps: self.history.len(),
                                    history: self.history.clone(),
                                });
                            }
                        }
                    }

                    let start_t = std::time::Instant::now();
                    let exec_res = CommandExecutor::execute_with_capture(cmd).await?;
                    let latency_ms = start_t.elapsed().as_millis();

                    let record = TaskStepRecord {
                        step_number: step_idx,
                        command: cmd.to_string(),
                        stdout: exec_res.stdout.clone(),
                        stderr: exec_res.stderr.clone(),
                        exit_code: exec_res.exit_code,
                        latency_ms,
                    };

                    // Salva também na sessão global do TAI
                    let mut session = SessionState::load_active();
                    session.record_full(
                        &format!("[Tarefa: {}] Passo {}", self.goal, step_idx),
                        cmd,
                        Some(&format!("{}ms", latency_ms)),
                        Some(format!("{}\n{}", exec_res.stdout, exec_res.stderr)),
                        Some(exec_res.exit_code),
                    );

                    self.history.push(record);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_decision_json_parse_continue() {
        let json = r#"{
            "status": "continue",
            "thought": "Preciso listar os arquivos primeiro.",
            "command": "ls -la /tmp",
            "explanation": "Lista os arquivos temporários",
            "affects_parent_shell": false
        }"#;

        let parsed = parse_task_decision(json).expect("should parse");
        assert_eq!(parsed.status, TaskStatus::Continue);
        assert_eq!(parsed.command.as_deref(), Some("ls -la /tmp"));
        assert_eq!(parsed.thought, "Preciso listar os arquivos primeiro.");
    }

    #[test]
    fn test_task_decision_json_parse_done() {
        let json = r#"```json
        {
            "status": "done",
            "thought": "Todos os pacotes foram instalados e validados.",
            "command": "",
            "explanation": "Instalação concluída com sucesso."
        }
        ```"#;

        let parsed = parse_task_decision(json).expect("should parse markdown json");
        assert_eq!(parsed.status, TaskStatus::Done);
        assert_eq!(parsed.explanation, "Instalação concluída com sucesso.");
    }

    #[test]
    fn test_task_decision_percent_fallback() {
        let raw = "%%%cargo test --bin tai%%%\nEXPLANATION: Executa a suíte de testes";
        let parsed = parse_task_decision(raw).expect("should parse percent fallback");
        assert_eq!(parsed.status, TaskStatus::Continue);
        assert_eq!(parsed.command.as_deref(), Some("cargo test --bin tai"));
    }

    #[test]
    fn test_deterministic_diagnostic_assessment() {
        let history = vec![
            TaskStepRecord {
                step_number: 1,
                command: "git status".to_string(),
                stdout: "On branch main".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
                latency_ms: 100,
            },
            TaskStepRecord {
                step_number: 2,
                command: "cargo build".to_string(),
                stdout: "".to_string(),
                stderr: "error[E0425]: cannot find value `foo` in this scope".to_string(),
                exit_code: 101,
                latency_ms: 1500,
            },
            TaskStepRecord {
                step_number: 3,
                command: "cargo test".to_string(),
                stdout: "".to_string(),
                stderr: "error: test failed".to_string(),
                exit_code: 1,
                latency_ms: 800,
            },
        ];

        let diag = TaskDiagnostic::deterministic_assess("corrigir build e rodar testes", &history);
        assert_eq!(diag.failed_steps_count, 2);
        assert_eq!(diag.difficulty, DifficultyLevel::Medium);
        assert!(diag.summary_of_errors.contains("cannot find value `foo`"));
    }

    #[test]
    fn test_task_runner_extension_callback() {
        let mut runner = TaskRunner::new("criar pasta e arquivo", 2);
        let mut extensions_called = 0;

        runner = runner.with_extension_callback(move |_diag| {
            extensions_called += 1;
            extensions_called <= 1
        });

        assert_eq!(runner.max_steps, 2);
        assert_eq!(runner.current_step, 0);
    }
}
