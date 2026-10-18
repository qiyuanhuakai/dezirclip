# Source this file from an interactive Bash session. Ctrl-G inserts clipboard text.
DZC_ADAPTER="${DZC_ADAPTER:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)/dzc-integrations.py}"
DZC_PYTHON="${DZC_PYTHON:-python3}"
DZC_PICKER="${DZC_PICKER:-fzf}"

_dzc_insert() {
    local selected status
    selected=$("$DZC_PYTHON" "$DZC_ADAPTER" pick --backend "$DZC_PICKER" --action insert
        status=$?
        printf '\036'
        exit "$status") || return
    selected=${selected%$'\036'}
    local LC_ALL=C
    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT}${selected}${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + ${#selected}))
}

if [[ $- == *i* ]]; then
    bind -x '"\C-g":_dzc_insert'
fi
