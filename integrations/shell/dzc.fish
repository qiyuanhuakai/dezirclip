# Source this file from Fish. Ctrl-G inserts selected text without executing it.
if not set -q DZC_ADAPTER
    set -g DZC_ADAPTER (path resolve (path dirname (status filename))/../dzc-integrations.py)
end
if not set -q DZC_PYTHON
    set -g DZC_PYTHON python3
end
if not set -q DZC_PICKER
    set -g DZC_PICKER fzf
end

function __dzc_insert
    set -l selected ($DZC_PYTHON "$DZC_ADAPTER" pick --backend "$DZC_PICKER" --action insert | string collect --no-trim-newlines)
    if test -n "$selected"
        commandline --insert -- "$selected"
    end
    commandline --function repaint
end

if status is-interactive
    bind \cg __dzc_insert
end
