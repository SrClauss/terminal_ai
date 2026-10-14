use directories::ProjectDirs;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum VenvError {
    #[error("Não foi possível determinar o diretório de dados do usuário (XDG_DATA_HOME)")]
    NoDataDir,
    #[error("Erro de I/O no gerenciamento da venv: {0}")]
    Io(#[from] std::io::Error),
    #[error("Comando '{0}' falhou com código {1}: {2}")]
    CommandFailed(String, i32, String),
    #[error("O executável python3 não foi encontrado no sistema host")]
    HostPythonNotFound,
    #[error("O ambiente virtual não existe em '{0}'")]
    VenvNotFound(PathBuf),
    #[error("Falha ao analisar saída do pip: {0}")]
    ParseError(String),
}

#[derive(Debug, Deserialize)]
struct PipPackageInfo {
    name: String,
    version: String,
}

#[derive(Debug, Clone)]
pub struct VenvManager {
    venv_path: PathBuf,
}

impl VenvManager {
    /// Inicializa o gerenciador com o caminho padrão persistente (~/.local/share/tai/venv)
    pub fn new() -> Result<Self, VenvError> {
        let proj_dirs = ProjectDirs::from("com", "terminal_ai", "tai")
            .ok_or(VenvError::NoDataDir)?;
        let data_dir = proj_dirs.data_local_dir();
        Ok(Self {
            venv_path: data_dir.join("venv"),
        })
    }

    /// Inicializa o gerenciador com um caminho customizado (útil para testes e ambientes isolados)
    pub fn with_path(path: PathBuf) -> Self {
        Self { venv_path: path }
    }

    /// Retorna a raiz da venv
    pub fn venv_path(&self) -> &Path {
        &self.venv_path
    }

    /// Retorna o caminho do binário Python da venv
    pub fn python_path(&self) -> PathBuf {
        self.venv_path.join("bin").join("python")
    }

    /// Retorna o caminho do binário Pip da venv
    pub fn pip_path(&self) -> PathBuf {
        self.venv_path.join("bin").join("pip")
    }

    /// Retorna o script activate da venv
    pub fn activate_path(&self) -> PathBuf {
        self.venv_path.join("bin").join("activate")
    }

    /// Verifica se a venv existe e possui o binário Python funcional
    pub fn exists(&self) -> bool {
        self.python_path().is_file()
    }

    /// Garante que o ambiente virtual exista, criando-o via `python3 -m venv` caso não exista
    pub fn ensure_venv(&self) -> Result<(), VenvError> {
        if self.exists() {
            return Ok(());
        }

        if let Some(parent) = self.venv_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let output = Command::new("python3")
            .args(["-m", "venv", self.venv_path.to_str().unwrap_or_default()])
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    VenvError::HostPythonNotFound
                } else {
                    VenvError::Io(e)
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(-1);
            return Err(VenvError::CommandFailed("python3 -m venv".to_string(), code, stderr));
        }

        // Garante que o pip básico e ferramentas de packaging estejam atualizados
        let pip = self.pip_path();
        if pip.exists() {
            let _ = Command::new(&pip)
                .args(["install", "--quiet", "--upgrade", "pip", "setuptools", "wheel"])
                .output();
        }

        Ok(())
    }

    /// Obtém a versão do Python instalada no ambiente virtual
    pub fn get_python_version(&self) -> Result<String, VenvError> {
        if !self.exists() {
            return Err(VenvError::VenvNotFound(self.venv_path.clone()));
        }

        let output = Command::new(self.python_path())
            .arg("--version")
            .output()?;

        if output.status.success() {
            let ver = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if ver.is_empty() {
                Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
            } else {
                Ok(ver)
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(-1);
            Err(VenvError::CommandFailed("python --version".to_string(), code, stderr))
        }
    }

    /// Calcula o tamanho total ocupado pelo diretório da venv em bytes
    pub fn get_disk_size(&self) -> u64 {
        if !self.venv_path.exists() {
            return 0;
        }

        Self::calc_dir_size(&self.venv_path)
    }

    fn calc_dir_size(path: &Path) -> u64 {
        let mut total = 0;
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if metadata.is_dir() {
                        total += Self::calc_dir_size(&entry.path());
                    } else {
                        total += metadata.len();
                    }
                }
            }
        }
        total
    }

    /// Formata bytes de forma legível para humanos (ex: 15.2 MB)
    pub fn format_disk_size(bytes: u64) -> String {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * 1024;
        const GB: u64 = 1024 * 1024 * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.1} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.1} KB", bytes as f64 / KB as f64)
        } else {
            format!("{bytes} B")
        }
    }

    /// Retorna a lista de pacotes instalados com nome e versão [(nome, versao)]
    pub fn list_installed_packages(&self) -> Result<Vec<(String, String)>, VenvError> {
        if !self.exists() {
            return Err(VenvError::VenvNotFound(self.venv_path.clone()));
        }

        let pip = self.pip_path();
        let output = Command::new(&pip)
            .args(["list", "--format=json"])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let code = output.status.code().unwrap_or(-1);
            return Err(VenvError::CommandFailed("pip list".to_string(), code, stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let packages: Vec<PipPackageInfo> = serde_json::from_str(&stdout)
            .map_err(|e| VenvError::ParseError(format!("{e}: {stdout}")))?;

        Ok(packages.into_iter().map(|p| (p.name, p.version)).collect())
    }

    /// Instala um ou mais pacotes na venv
    pub fn install_packages(&self, packages: &[&str]) -> Result<String, VenvError> {
        self.ensure_venv()?;
        if packages.is_empty() {
            return Ok(String::new());
        }

        let pip = self.pip_path();
        let mut cmd = Command::new(&pip);
        cmd.arg("install");
        for p in packages {
            cmd.arg(p);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(VenvError::CommandFailed(
                format!("pip install {}", packages.join(" ")),
                code,
                format!("{}\n{}", stdout, stderr),
            ));
        }

        Ok(stdout)
    }

    /// Limpa completamente o ambiente virtual (remove todos os pacotes e arquivos) e recria uma venv nova e limpa
    pub fn clean(&self) -> Result<(), VenvError> {
        if self.venv_path.exists() {
            fs::remove_dir_all(&self.venv_path)?;
        }
        self.ensure_venv()?;
        Ok(())
    }

    /// Atualiza em massa as ferramentas base (pip, setuptools, wheel) e todos os pacotes instalados na venv
    pub fn update(&self) -> Result<VenvUpdateResult, VenvError> {
        self.ensure_venv()?;

        let mut targets = vec!["pip".to_string(), "setuptools".to_string(), "wheel".to_string()];
        if let Ok(packages) = self.list_installed_packages() {
            for (pkg, _) in packages {
                let lower = pkg.to_lowercase();
                if lower != "pip" && lower != "setuptools" && lower != "wheel" {
                    targets.push(pkg);
                }
            }
        }

        let pip = self.pip_path();
        let mut cmd = Command::new(&pip);
        cmd.arg("install").arg("--upgrade");
        for t in &targets {
            cmd.arg(t);
        }

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(VenvError::CommandFailed(
                format!("pip install --upgrade {}", targets.join(" ")),
                code,
                format!("{}\n{}", stdout, stderr),
            ));
        }

        let details = if !stdout.trim().is_empty() {
            stdout
        } else {
            stderr
        };

        Ok(VenvUpdateResult {
            updated_packages: targets,
            details,
        })
    }
}

#[derive(Debug, Clone)]
pub struct VenvUpdateResult {
    pub updated_packages: Vec<String>,
    pub details: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_venv_paths() {
        let manager = VenvManager::with_path(PathBuf::from("/tmp/tai_test_venv"));
        assert_eq!(manager.venv_path(), Path::new("/tmp/tai_test_venv"));
        assert_eq!(
            manager.python_path(),
            PathBuf::from("/tmp/tai_test_venv/bin/python")
        );
        assert_eq!(
            manager.pip_path(),
            PathBuf::from("/tmp/tai_test_venv/bin/pip")
        );
        assert_eq!(
            manager.activate_path(),
            PathBuf::from("/tmp/tai_test_venv/bin/activate")
        );
    }

    #[test]
    fn test_format_disk_size() {
        assert_eq!(VenvManager::format_disk_size(500), "500 B");
        assert_eq!(VenvManager::format_disk_size(2048), "2.0 KB");
        assert_eq!(VenvManager::format_disk_size(15 * 1024 * 1024), "15.0 MB");
    }

    #[test]
    fn test_create_and_query_venv() {
        let temp_dir = tempdir().expect("Falha ao criar diretório temporário");
        let venv_dir = temp_dir.path().join("venv");
        let manager = VenvManager::with_path(venv_dir);

        assert!(!manager.exists());
        let res = manager.ensure_venv();
        assert!(res.is_ok(), "Falha ao criar venv: {:?}", res);
        assert!(manager.exists());

        let ver = manager.get_python_version();
        assert!(ver.is_ok(), "Falha ao obter versão do Python");
        let version_str = ver.unwrap();
        assert!(version_str.starts_with("Python 3."));

        let pkgs = manager.list_installed_packages();
        assert!(pkgs.is_ok(), "Falha ao listar pacotes instalados");
        let package_list = pkgs.unwrap();
        assert!(
            package_list.iter().any(|(name, _)| name.to_lowercase() == "pip"),
            "pip deve estar instalado na venv criada"
        );

        let size = manager.get_disk_size();
        assert!(size > 0, "O tamanho da venv em disco deve ser maior que 0");
    }

    #[test]
    fn test_clean_venv() {
        let temp_dir = tempdir().expect("Falha ao criar diretório temporário");
        let venv_dir = temp_dir.path().join("venv");
        let manager = VenvManager::with_path(venv_dir.clone());

        manager.ensure_venv().expect("Falha ao criar venv inicial");
        let marker_file = venv_dir.join("marker_test.txt");
        fs::write(&marker_file, "teste de limpeza").expect("Falha ao gravar marker");
        assert!(marker_file.exists());

        let clean_res = manager.clean();
        assert!(clean_res.is_ok(), "Falha ao limpar venv: {:?}", clean_res);
        assert!(!marker_file.exists(), "Marker não deve mais existir após clean");
        assert!(manager.exists(), "Venv deve existir recriada limpa após clean");
    }

    #[test]
    fn test_install_packages_empty() {
        let temp_dir = tempdir().expect("Falha ao criar diretório temporário");
        let venv_dir = temp_dir.path().join("venv");
        let manager = VenvManager::with_path(venv_dir);
        let res = manager.install_packages(&[]);
        assert!(res.is_ok());
    }
}
