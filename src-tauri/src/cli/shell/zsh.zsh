# Load with: eval "$(dzc zsh)". Ctrl-G inserts text without submitting it.
if [[ -z ${DZC_BIN:-} ]]; then typeset -g DZC_BIN=__DZC_EXECUTABLE__; fi
typeset -g DZC_PICKER="${DZC_PICKER:-tui}"

_dzc_insert() {
    local selected result
    selected=$(if [[ $DZC_PICKER == tui ]]; then
            "$DZC_BIN" tui --action insert
        else
            "$DZC_BIN" pick --backend "$DZC_PICKER" --action insert
        fi
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
