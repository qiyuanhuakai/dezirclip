# Load with: dzc fish | source. Ctrl-G inserts text without submitting it.
if not set -q DZC_BIN; or test -z "$DZC_BIN"
    set -g DZC_BIN __DZC_EXECUTABLE__
end
if not set -q DZC_PICKER; or test -z "$DZC_PICKER"
    set -g DZC_PICKER tui
end

function __dzc_select
    if test "$DZC_PICKER" = tui
        $DZC_BIN tui --action insert
    else
        $DZC_BIN pick --backend "$DZC_PICKER" --action insert
    end
end

function __dzc_insert
    set -l selected (__dzc_select | string collect --no-trim-newlines)
    set -l selection_status $pipestatus[1]
    if test "$selection_status" -eq 0; and test -n "$selected"
        commandline --insert -- "$selected"
    end
    commandline --function repaint
end

if status is-interactive
    bind \cg __dzc_insert
end
