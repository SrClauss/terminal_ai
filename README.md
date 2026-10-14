<div align="center">

# ⚡ Terminal AI (`tai`)

**Traduza comandos em linguagem natural diretamente para Bash/Linux com alta performance, didática prévia e segurança contra desastres.**

[![Rust](https://img.shields.io/badge/Language-Rust-orange.svg?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](https://github.com/SrClauss/terminal_ai/pulls)
[![GitHub stars](https://img.shields.io/github/stars/SrClauss/terminal_ai?style=social)](https://github.com/SrClauss/terminal_ai)

[Instalação](#-instalação-rápida) • [Como Usar](#-como-usar) • [Segurança](#-segurança-e-trava-contra-desastres) • [Como Contribuir](#-como-contribuir-roadmap-aberto)

</div>

---

## 🎯 Por que o `tai`?

Quantas vezes você precisou pesquisar as flags complexas do `tar`, encontrar processos que estão consumindo muita memória com `ps` ou filtrar arquivos no `find`? 

O **Terminal AI (`tai`)** foi desenvolvido nativamente em **Rust** para ser um copiloto direto no seu terminal:
1. **Ultrarrápido**: Inicialização em **< 2ms** (sem delays de interpretador).
2. **Explica Antes de Rodar**: Você nunca executa um comando às cegas. O `tai` detalha cada flag e pipe gerado.
3. **Trava de Segurança Nativa**: Bloqueia e emite alertas em vermelho contra comandos destrutivos (`rm -rf /`, `mkfs`, `> /dev/sd*`).
4. **Área de Transferência Nativamente Integrada**: Suporte a Wayland (`wl-copy`) e X11 (`xclip`).
5. **Multiprovedores**: Conecte com **DeepSeek**, **OpenAI** ou **Ollama local** (`yi-coder:1.5b`, `qwen2.5-coder`) para trabalhar 100% offline.
6. **Memória de Sessão Contínua**: Lembra o contexto dos seus comandos recentes (TTL de 15 min). Diga *"agora compacte eles"* ou *"adicione a flag -v"* e ele sabe exatamente o que fazer.
7. **Multicomandos Encadeados**: Gera sequências completas (`&&`, `|`, `;`) destacando cada passo visualmente na tela. Flag `-n` ou `tai clear` para reiniciar a memória.
8. **Streaming em Tempo Real (SSE)**: Respostas e comandos exibidos progressivamente em tempo real conforme chegam da IA, reduzindo a latência percebida para quase zero.

---

## 📦 Instalação Rápida

Basta clonar e rodar o instalador automatizado de 1 comando:

```bash
git clone https://github.com/SrClauss/terminal_ai.git
cd terminal_ai
./install.sh
```

O script compilará a versão release otimizada, criará os comandos globais `tai` e `termai` em `~/.local/bin/` e configurará o seu `$PATH`.

---

## 🚀 Como Usar

### 1. Modo Interativo Assistido (Padrão)
```bash
tai "listar todas as portas tcp abertas e os processos usando"
```
O `tai` exibe o comando gerado, explica didaticamente o que cada parâmetro faz e abre o menu de confirmação:
```text
────────────────────────────────────────────────────────────
Comando Sugerido: lsof -iTCP -sTCP:LISTEN -n -P
────────────────────────────────────────────────────────────

Explicação detalhada:
  - lsof: lista conexões de rede e arquivos abertos.
  - -iTCP: restringe o filtro para conexões TCP.
  - -sTCP:LISTEN: apenas portas que estão ativamente em escuta.
  - -n -P: impede resolução lenta de DNS e nomes de portas.

? O que deseja fazer?
> Executar agora
  Copiar para área de transferência
  Cancelar
```

### 2. Modo Rápido / Direto (`-y` / `--yes`)
Executa imediatamente no shell (com trava de segurança ativa):
```bash
tai -y "limpar o cache de pacotes do apt"
```

### 3. Modo Dry-Run / Pipe (`-p` / `--print`)
Gera apenas o comando no `stdout` sem menus nem animações, ideal para scripts ou `eval`:
```bash
eval "$(tai -p 'mostrar uso de disco apenas de sistemas de arquivos reais')"
```

### 4. Modo Copiar para o Clipboard (`-c` / `--copy`)
```bash
tai -c "compactar a pasta src excluindo .git e target"
```

### 5. Desativar Streaming Sob Demanda (`--no-stream`)
Por padrão, o `tai` exibe o comando gerado em tempo real via streaming SSE. Para forçar a resposta em bloco único:
```bash
tai --no-stream "descompactar arquivo tar.gz"
```

### 6. Modo de Tarefa em Loop Autônomo (`tai task`)
Para objetivos complexos que exigem múltiplos passos consecutivos (ex: compilar, diagnosticar e testar), o `tai` possui um motor de loop agêntico:
```bash
tai task "instalar dependências do projeto e rodar a suíte de testes"
```
* **Checkpoint com Diagnóstico de Dificuldade**: Ao atingir o limite de passos (padrão de 5 passos ou `--max-steps <N>`), a IA compila os erros ocorridos, avalia a complexidade/dificuldade e pergunta interativamente se você deseja conceder **mais 5 passos**.
* **Modo Totalmente Autônomo (`-y`)**: Execute sequencialmente os passos sem confirmação intermediária:
```bash
tai task "preparar ambiente de build e compilar binário" -y
```

---

## ⚙️ Configuração

O `tai` pode ser configurado tanto por **comandos de terminal** quanto editando diretamente os **arquivos de configuração**:

### 1. Via Comandos de Terminal (`tai config`)

```bash
# Alternar automaticamente para Ollama local (offline):
tai config set provider ollama
# (O tai atualizará automaticamente api_url para http://localhost:11434/v1 e o modelo padrão)

# Para usar um modelo específico do seu Ollama:
tai config set model "yi-coder:1.5b"

# Alternar de volta para a DeepSeek na nuvem:
tai config set provider deepseek
tai config set api_key "sua-chave-aqui"

# Ativar ou desativar streaming de respostas (padrão: true):
tai config set stream false

# Ver a configuração ativa:
tai config show
```

### 2. Diretamente no Arquivo (`~/.config/tai/config.toml`)

O arquivo de configuração permanente fica localizado em:
`~/.config/tai/config.toml` (permissão de segurança `0600`)

```toml
provider = "ollama"
model = "yi-coder:1.5b"
api_url = "http://localhost:11434/v1"
api_key = "sk-..."
language = "pt"
default_mode = "interactive"
stream = true
```

### 3. Regras Pessoais e Preferências Fixas (`~/.config/tai/rules.md`)

Você pode ensinar preferências permanentes ao `tai` criando ou editando o arquivo `~/.config/tai/rules.md`:

```markdown
# Minhas Preferências para o tai
- Sempre que sugerir comandos de disco (du, df), adicione a flag -h para legibilidade humana.
- Prefira neovim (nvim) em vez de nano se for abrir editores.
- Sempre liste arquivos ordenando por tamanho ou data mais recente.
```
O `tai` lerá esse arquivo automaticamente e obedecerá às suas instruções em qualquer consulta futura.

---

## 🛡️ Segurança e Trava Contra Desastres

O `tai` possui um analisador estático interno de comandos. Caso uma LLM gere acidentalmente um comando com potencial catastrófico:
- `rm -rf /` ou `rm -rf /*`
- `mkfs.*` em partições de sistema
- Redirecionamentos diretos em `/dev/sd*` ou `/dev/nvme*`
- Fork bombs

O comando é **imediatamente marcado com alerta vermelho crítico** e a execução direta (`-y`) é cancelada para evitar perda de dados.

---

## 🤝 Como Contribuir (Precisamos da sua ajuda!)

Este é um projeto **Open Source** e qualquer contribuição é muito bem-vinda! Se você achou a ferramenta útil, ajude com uma ⭐ no GitHub e colabore conosco.

### Áreas onde você pode ajudar agora:

- [ ] **Novos Provedores de IA**: Integração com Anthropic Claude, Google Gemini e Groq.
- [ ] **Empacotamento de Distribuições**:
  - Criar pacote para o Arch Linux AUR (`PKGBUILD`).
  - Homebrew tap para Linux/macOS.
  - Pacote Debian/Ubuntu (`.deb`).
- [ ] **Testes e Benchmarks com Modelos Locais (Ollama)**:
  - > ⚠️ **Nota Prática sobre Modelos Pequenos**: Testes realizados com SLMs ultraleves (~0.8B a 1.5B, como `yi-coder:1.5b` e `dgpl-linux-assistant`) apresentaram resultados insatisfatórios em produção (latência alta em CPU, alucinações de delimitadores e dificuldade em seguir formatos estritos).
  - Convidamos os contribuidores a testar e homologar modelos mais robustos (7B a 14B, como `qwen2.5-coder:7b`, `deepseek-coder:6.7b` ou `codellama`) para uso local offline de alta qualidade.
- [ ] **Internacionalização (i18n)**:
  - Adicionar suporte a Espanhol (`es`), Francês (`fr`), Alemão (`de`), etc.
- [ ] **Suporte a mais Shells**:
  - Autocompletions para `fish`, `zsh` e `bash`.

### Como enviar sua contribuição:
1. Faça um Fork do projeto (`https://github.com/SrClauss/terminal_ai`).
2. Crie sua branch de feature: `git checkout -b feature/minha-melhoria`.
3. Desenvolva e teste com `cargo test`.
4. Envie seu Pull Request! Responderemos e revisaremos com muito prazer.

---

## 🔄 Atualização Automática

Para manter sua instalação sempre na versão mais recente:
```bash
tai update
```

---

## 📝 Licença
Distribuído sob a licença MIT. Consulte `LICENSE` para mais informações.
