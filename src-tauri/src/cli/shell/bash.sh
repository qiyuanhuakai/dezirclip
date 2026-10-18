# Bash 5+. Load with: eval "$(dzc bash)". Ctrl-G inserts text without submitting it.
if [[ -z ${DZC_BIN:-} ]]; then DZC_BIN=__DZC_EXECUTABLE__; fi
DZC_PICKER="${DZC_PICKER:-tui}"

_dzc_insert() {
    if (( BASH_VERSINFO[0] < 5 )); then
        printf 'dzc Bash integration requires Bash 5 or newer.\n' >&2
        return 1
    fi
    local selected result
    selected=$(if [[ $DZC_PICKER == tui ]]; then
            "$DZC_BIN" tui --action insert
        else
            "$DZC_BIN" pick --backend "$DZC_PICKER" --action insert
        fi
        result=$?
        printf '\036'
        exit "$result") || return
    selected=${selected%$'\036'}
    READLINE_LINE="${READLINE_LINE:0:READLINE_POINT}${selected}${READLINE_LINE:READLINE_POINT}"
    READLINE_POINT=$((READLINE_POINT + ${#selected}))
}

if [[ $- == *i* ]]; then
    bind -x '"\C-g":_dzc_insert'
fi
