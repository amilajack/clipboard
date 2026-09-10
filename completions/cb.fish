# Completion for cb in fish. Save it where fish looks for completions:
#   cb completions fish > ~/.config/fish/completions/cb.fish

# cb takes a single argument, so files are only offered for the first one.
complete -c cb -f
complete -c cb -n __fish_use_subcommand -F
complete -c cb -n __fish_use_subcommand -a peek -d 'Search clipboard history and copy an entry again'
complete -c cb -n __fish_use_subcommand -a watch -d 'Record everything copied, checking every 2 seconds'
complete -c cb -n __fish_use_subcommand -a completions -d 'Print the completion script for a shell'
complete -c cb -n __fish_use_subcommand -s h -l help -d 'Print help'
complete -c cb -n __fish_use_subcommand -s V -l version -d 'Print version'
complete -c cb -n '__fish_seen_subcommand_from completions; and not __fish_seen_subcommand_from bash zsh fish' -a 'bash zsh fish'
complete -c cb -n '__fish_seen_subcommand_from watch; and not __fish_seen_argument -l install -l uninstall' -l install -d 'Also start watching whenever you log in'
complete -c cb -n '__fish_seen_subcommand_from watch; and not __fish_seen_argument -l install -l uninstall' -l uninstall -d 'Stop watching, now and at login'
