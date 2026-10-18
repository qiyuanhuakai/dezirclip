# Source in Nushell 0.116+. Ctrl-G inserts selected text at the current cursor.
const dzc_default_adapter = path self ../dzc-integrations.py

$env.DZC_ADAPTER = ($env.DZC_ADAPTER? | default $dzc_default_adapter)
$env.DZC_PYTHON = ($env.DZC_PYTHON? | default (if $nu.os-info.name == 'windows' { 'python' } else { 'python3' }))
$env.DZC_PICKER = ($env.DZC_PICKER? | default 'fzf')

def dzc-insert [] {
    let result = (try {
        do { ^$env.DZC_PYTHON $env.DZC_ADAPTER pick --backend $env.DZC_PICKER --action insert } | complete
    } catch { null })
    if $result == null { return }
    if $result.exit_code != 0 { return }
    commandline edit --insert $result.stdout
}

$env.config.keybindings = (
    $env.config.keybindings
    | where { |binding| $binding.name? != 'dzc_insert' }
    | append {
        name: dzc_insert
        modifier: control
        keycode: char_g
        mode: [emacs vi_insert vi_normal]
        event: { send: executehostcommand, cmd: 'dzc-insert' }
    }
)
