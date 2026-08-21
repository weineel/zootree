# Keep config recovery commands bootstrap-safe

`zootree config path`, `show`, and `edit` run before global configuration parsing and file logging initialization so they remain available when `config.toml` is missing or malformed. This deliberately separates recovery commands from the normal startup path: `config agents` still uses parsed global configuration, while the recovery commands only locate, read, or repair its source file. Preserving this boundary prevents a uniform-startup refactor from making a broken configuration impossible to diagnose or fix with zootree itself.
