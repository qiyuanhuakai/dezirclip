# Source this file from Zsh. Ctrl-G inserts clipboard text into the current buffer.
typeset -g DZC_ADAPTER="${DZC_ADAPTER:-${${(%):-%x}:A:h:h}/dzc-integrations.py}"
typeset -g DZC_PYTHON="${DZC_PYTHON:-python3}"
typeset -g DZC_PICKER="${DZC_PICKER:-fzf}"

_dzc_insert() {
    local selected result
    selected=$("$DZC_PYTHON" "$DZC_ADAPTER" pick --backend "$DZC_PICKER" --action insert
        result=$?
        printf '\036'
        exit "$result") || { zle redisplay; return; }
    selected=${selected%$'\036'}
    LBUFFER+="$selected"
    zle redisplay
}

if [[ -o interactive ]]; then
    zle -N _dzc_insert
    bindkey '^G' _dzc_insert
fi
