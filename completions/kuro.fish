# fish completions for kuro:  cp completions/kuro.fish ~/.config/fish/completions/   (or: make install)
complete -c kuro -l config -r -F -d 'Read this config.toml'
complete -c kuro -l version -d 'Print version'
complete -c kuro -s h -l help -d 'Show help'
complete -c kuro -F
