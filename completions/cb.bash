# Completion for cb in bash. Load it by adding this line to ~/.bashrc:
#   eval "$(cb completions bash)"
#
# Sticks to what bash 3.2, the version macOS ships, supports.

_cb() {
    local cur=$2 prev=$3
    COMPREPLY=()
    case $COMP_CWORD in
    1)
        if [[ $cur == -* ]]; then
            COMPREPLY=($(compgen -W '-h --help -V --version' -- "$cur"))
        else
            COMPREPLY=($(compgen -W 'peek watch completions' -- "$cur"))
            # Read line by line so names with spaces stay whole.
            local file
            while IFS= read -r file; do
                COMPREPLY+=("$file")
            done < <(compgen -f -- "$cur")
        fi
        ;;
    2)
        if [[ $prev == completions ]]; then
            COMPREPLY=($(compgen -W 'bash zsh fish' -- "$cur"))
        elif [[ $prev == watch ]]; then
            COMPREPLY=($(compgen -W '--install --uninstall' -- "$cur"))
        fi
        ;;
    esac
}

# `filenames` quotes special characters and marks directories with a slash.
complete -o filenames -F _cb cb
