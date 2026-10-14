use colored::Colorize;

#[derive(Debug, PartialEq, Eq)]
pub enum DangerLevel {
    /// Seguro para execução rotineira (ex: ls, grep, ps)
    Safe,
    /// Risco moderado: alteração ou remoção de arquivos específicos (ex: rm *.tmp, kill)
    Moderate,
    /// Risco catastrófico: perigo iminente de perda irreversível de dados ou do sistema operacional
    Catastrophic,
}

pub struct SafetyAssessment {
    pub level: DangerLevel,
    pub reason: Option<String>,
}

pub struct SafetyAnalyzer;

impl SafetyAnalyzer {
    pub fn assess(command: &str) -> SafetyAssessment {
        let trimmed = command.trim();
        let lower = trimmed.to_lowercase();

        // 1. Padrões Catastróficos (bloqueio de execução cega e alerta vermelho)
        let catastrophic_patterns = [
            ("rm -rf /", "Tentativa de apagar recursivamente o diretório raiz"),
            ("rm -rf /*", "Tentativa de apagar todos os diretórios do sistema"),
            ("mkfs", "Comando de formatação de sistema de arquivos"),
            ("> /dev/sd", "Tentativa de sobrescrita direta em disco rígido físico"),
            ("> /dev/nvme", "Tentativa de sobrescrita direta em drive NVMe"),
            ("dd if=", "Comando de baixo nível com perigo de sobrescrever partições"),
            (":(){ :|:& };:", "Fork bomb detectada (travaria o Linux)"),
            ("chmod -r 777 /", "Quebra generalizada de permissões do sistema"),
            ("chown -r", "Alteração recursiva de proprietário em massa"),
        ];

        for (pattern, reason) in catastrophic_patterns {
            if lower.contains(pattern) {
                return SafetyAssessment {
                    level: DangerLevel::Catastrophic,
                    reason: Some(reason.to_string()),
                };
            }
        }

        // Se a solicitação for Python/Pip executado dentro da venv (fixa ou efêmera)
        // e não contiver padrões catastróficos, ela é aceita com segurança (DangerLevel::Safe)
        let is_venv_execution = (lower.contains("tai/venv")
            || lower.contains("/venv/bin/")
            || lower.contains("$tmpvenv")
            || lower.contains("tai venv"))
            && (lower.contains("python") || lower.contains("pip") || lower.contains("clean") || lower.contains("update"));

        if is_venv_execution {
            return SafetyAssessment {
                level: DangerLevel::Safe,
                reason: None,
            };
        }

        // 2. Padrões Moderados (remoções e ações administrativas)
        let moderate_patterns = [
            ("rm ", "Comando de remoção de arquivos"),
            ("rmdir ", "Comando de remoção de diretório"),
            ("kill -9", "Encerramento forçado de processo"),
            ("reboot", "Reinicialização da máquina"),
            ("shutdown", "Desligamento da máquina"),
            ("iptables -f", "Liberação ou limpeza de firewall"),
            ("systemctl stop", "Parada de serviço do sistema"),
        ];

        for (pattern, reason) in moderate_patterns {
            if lower.contains(pattern) {
                return SafetyAssessment {
                    level: DangerLevel::Moderate,
                    reason: Some(reason.to_string()),
                };
            }
        }

        SafetyAssessment {
            level: DangerLevel::Safe,
            reason: None,
        }
    }

    pub fn print_warning(assessment: &SafetyAssessment) {
        match assessment.level {
            DangerLevel::Catastrophic => {
                eprintln!(
                    "{}",
                    "╔═══════════════════════════════════════════════════════════════╗"
                        .red()
                        .bold()
                );
                eprintln!(
                    "{}",
                    "║               ALERTA DE RISCO CRÍTICO E CATASTRÓFICO           ║"
                        .red()
                        .bold()
                );
                eprintln!(
                    "{}",
                    "╚═══════════════════════════════════════════════════════════════╝"
                        .red()
                        .bold()
                );
                if let Some(ref r) = assessment.reason {
                    eprintln!("{} {}", "Motivo:".red().bold(), r.red());
                }
                eprintln!(
                    "{}",
                    "ATENÇÃO: A execução direta desassistida (-y) foi cancelada para este comando!"
                        .yellow()
                        .bold()
                );
            }
            DangerLevel::Moderate => {
                if let Some(ref r) = assessment.reason {
                    println!("{} {}", "Aviso de impacto:".yellow().bold(), r.yellow());
                }
            }
            DangerLevel::Safe => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_catastrophic_rm_rf_root() {
        let res = SafetyAnalyzer::assess("sudo rm -rf /");
        assert_eq!(res.level, DangerLevel::Catastrophic);
    }

    #[test]
    fn test_detect_catastrophic_mkfs() {
        let res = SafetyAnalyzer::assess("sudo mkfs.ext4 /dev/sda1");
        assert_eq!(res.level, DangerLevel::Catastrophic);
    }

    #[test]
    fn test_detect_catastrophic_dd() {
        let res = SafetyAnalyzer::assess("dd if=/dev/zero of=/dev/sdb bs=1M");
        assert_eq!(res.level, DangerLevel::Catastrophic);
    }

    #[test]
    fn test_detect_moderate_rm() {
        let res = SafetyAnalyzer::assess("rm -f *.tmp");
        assert_eq!(res.level, DangerLevel::Moderate);
    }

    #[test]
    fn test_safe_command() {
        let res = SafetyAnalyzer::assess("ls -la && ps aux | grep python");
        assert_eq!(res.level, DangerLevel::Safe);
    }

    #[test]
    fn test_venv_python_commands_are_safe() {
        let res1 = SafetyAnalyzer::assess("~/.local/share/tai/venv/bin/python script.py");
        assert_eq!(res1.level, DangerLevel::Safe);

        let res2 = SafetyAnalyzer::assess("~/.local/share/tai/venv/bin/pip install requests");
        assert_eq!(res2.level, DangerLevel::Safe);

        let res3 = SafetyAnalyzer::assess("TMPVENV=$(mktemp -d) && python3 -m venv \"$TMPVENV\" && \"$TMPVENV/bin/python\" -c 'print(1)' && rm -rf \"$TMPVENV\"");
        assert_eq!(res3.level, DangerLevel::Safe);
    }
}
