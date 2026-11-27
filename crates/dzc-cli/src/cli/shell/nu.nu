# Nushell 0.116+. Install in $nu.user-autoload-dirs.0, then restart Nushell.
$env.DZC_BIN = ($env.DZC_BIN? | default __DZC_EXECUTABLE__)
$env.DZC_PICKER = ($env.DZC_PICKER? | default 'tui')

def dzc-insert [] {
    let arguments = if $env.DZC_PICKER == 'tui' {
        ['tui' '--action' 'insert']
    } else {
        ['pick' '--backend' $env.DZC_PICKER '--action' 'insert']
    }
    let result = (try {
        do { ^$env.DZC_BIN ...$arguments } | complete
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
