#!/usr/bin/env bash
set -e

# Cores para o output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}==========================================${NC}"
echo -e "${BLUE}    Instalador do Terminal AI (tai)      ${NC}"
echo -e "${BLUE}==========================================${NC}"

TARGET_DIR="$HOME/.local/bin"
mkdir -p "$TARGET_DIR"

echo -e "\n${YELLOW}[1/4] Compilando binário nativo em modo release...${NC}"
cargo build --release

echo -e "\n${YELLOW}[2/4] Instalando binário em $TARGET_DIR...${NC}"
install -m 755 target/release/tai "$TARGET_DIR/tai"

echo -e "\n${YELLOW}[3/4] Criando atalho/alias 'termai'...${NC}"
ln -sf "$TARGET_DIR/tai" "$TARGET_DIR/termai"

echo -e "\n${YELLOW}[4/4] Verificando configuração do PATH e Shell Hook...${NC}"
PATH_EXPORT='export PATH="$HOME/.local/bin:$PATH"'

SHELL_HOOK_BLOCK='# Terminal AI (tai) Shell Hook para mutação da sessão atual (cd, export, source)
tai() {
    local tmpfile
    tmpfile=$(mktemp /tmp/tai_cmd.XXXXXX)
    command tai --shell-hook "$tmpfile" "$@"
    local exit_code=$?
    if [ -s "$tmpfile" ]; then
        local cmd_to_run
        cmd_to_run=$(cat "$tmpfile")
        rm -f "$tmpfile"
        eval "$cmd_to_run"
    else
        rm -f "$tmpfile"
    fi
    return $exit_code
}'

update_shell_profile() {
    local profile_file="$1"
    if [ -f "$profile_file" ]; then
        if ! grep -q '\.local/bin' "$profile_file"; then
            echo "" >> "$profile_file"
            echo "# Adicionado pelo instalador do Terminal AI (tai)" >> "$profile_file"
            echo "$PATH_EXPORT" >> "$profile_file"
            echo -e "${GREEN}✓ PATH adicionado a $profile_file${NC}"
        else
            echo -e "${GREEN}✓ $profile_file já contém ~/.local/bin no PATH${NC}"
        fi

        if ! grep -q 'Terminal AI (tai) Shell Hook' "$profile_file"; then
            echo "" >> "$profile_file"
            echo "$SHELL_HOOK_BLOCK" >> "$profile_file"
            echo -e "${GREEN}✓ Shell Hook (mutação de terminal) instalado em $profile_file${NC}"
        else
            echo -e "${GREEN}✓ $profile_file já contém o Shell Hook do tai${NC}"
        fi
    fi
}

update_shell_profile "$HOME/.bashrc"
update_shell_profile "$HOME/.zshrc"

echo -e "\n${GREEN}==========================================${NC}"
echo -e "${GREEN}  ✓ Instalação concluída com sucesso!     ${NC}"
echo -e "${GREEN}==========================================${NC}"
echo -e "Você já pode usar os comandos:"
echo -e "  ${BLUE}tai \"listar arquivos grandes ordenados por tamanho\"${NC}"
echo -e "  ${BLUE}termai \"mostrar portas tcp abertas\"${NC}"
echo -e "  ${BLUE}tai --help${NC}"
