use crate::vmf::VmfAction;

#[derive(Debug)]
pub struct VamoraError {
    pub code: i32,
    pub message: String,
}

impl VamoraError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

#[derive(Debug)]
pub enum SettingsAction {
    Put,
    Get,
    Delete,
    List,
}

#[derive(Debug)]
pub enum Command {
    Settings {
        action: SettingsAction,
        /// The category directory (e.g. "theme")
        category: String,
        /// Optional sub-category — if Some("colors"), file is colors.vmf
        /// If None, file is settings.vmf
        subcategory: Option<String>,
        key: Option<String>,
        value: Option<String>,
        system: bool,
    },
    Vmf {
        file: String,
        action: VmfAction,
        key: String,
        value: Option<String>,
    },
    Check {
        category: String,
        subcategory: Option<String>,
    },
    Exec {
        name: String,
    },
    Utility {
        name: String,
    },
    Info,
    Help,
}

pub struct Cli {
    pub command: Command,
}

impl Cli {
    pub fn parse_args() -> Result<Self, VamoraError> {
        let args: Vec<String> = std::env::args().skip(1).collect();

        if args.is_empty() || args[0] == "--help" || args[0] == "-h" || args[0] == "help" {
            return Ok(Cli { command: Command::Help });
        }

        match args[0].as_str() {
            "info" => Ok(Cli { command: Command::Info }),

            "check" => {
                // vamorasys check <category> [subcategory]
                let category = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys check <category> [subcategory]")
                })?.clone();
                let subcategory = args.get(2).cloned();
                Ok(Cli { command: Command::Check { category, subcategory } })
            }

            "exec" => {
                let name = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys exec <binary>")
                })?.clone();
                Ok(Cli { command: Command::Exec { name } })
            }

            "utility" => {
                let name = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys utility <utility>")
                })?.clone();
                Ok(Cli { command: Command::Utility { name } })
            }

            "vmf" => {
                // vamorasys vmf <file> <action> [key] [value]
                let file = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys vmf <file> <action> [key] [value]")
                })?.clone();
                let action_str = args.get(2).ok_or_else(|| {
                    VamoraError::new(1, "missing action: put | get | delete | list")
                })?;

                if action_str == "list" {
                    return Ok(Cli {
                        command: Command::Vmf {
                            file,
                            action: VmfAction::List,
                            key: String::new(),
                            value: None,
                        },
                    });
                }

                let action = match action_str.as_str() {
                    "put"    => VmfAction::Put,
                    "get"    => VmfAction::Get,
                    "delete" => VmfAction::Delete,
                    other    => return Err(VamoraError::new(1,
                        format!("unknown vmf action '{}': use put | get | delete | list", other))),
                };

                let key = args.get(3).ok_or_else(|| {
                    VamoraError::new(1, "missing key argument")
                })?.clone();
                let value = args.get(4).cloned();

                if matches!(action, VmfAction::Put) && value.is_none() {
                    return Err(VamoraError::new(1,
                        "put requires a value: vamorasys vmf <file> put <key> <value>"));
                }

                Ok(Cli { command: Command::Vmf { file, action, key, value } })
            }

            "settings" => {
                // Strip --system flag first
                let system = args.contains(&"--system".to_string());
                let filtered: Vec<&String> = args.iter()
                    .filter(|a| a.as_str() != "--system")
                    .collect();

                // filtered[0] = "settings"
                // filtered[1] = action
                // filtered[2] = category
                // filtered[3] = subcategory OR key  (disambiguated by action + count)
                // filtered[4] = key OR value
                // filtered[5] = value (put with subcategory)

                let action_str = filtered.get(1).ok_or_else(|| {
                    VamoraError::new(1, "missing action: put | get | delete | list")
                })?;

                let action = match action_str.as_str() {
                    "put"    => SettingsAction::Put,
                    "get"    => SettingsAction::Get,
                    "delete" => SettingsAction::Delete,
                    "list"   => SettingsAction::List,
                    other    => return Err(VamoraError::new(1,
                        format!("unknown action '{}': use put | get | delete | list", other))),
                };

                let category = filtered.get(2).ok_or_else(|| {
                    VamoraError::new(1, "missing category argument")
                })?.to_string();

                match action {
                    // list: vamorasys settings list <category> [subcategory]
                    SettingsAction::List => {
                        let subcategory = filtered.get(3).map(|s| s.to_string());
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                subcategory,
                                key: None,
                                value: None,
                                system,
                            },
                        })
                    }

                    // get/delete: vamorasys settings get <category> [subcategory] <key>
                    // We distinguish subcategory vs key by whether there are 2 or 1 remaining args.
                    SettingsAction::Get | SettingsAction::Delete => {
                        let remaining: Vec<&str> = filtered[3..].iter().map(|s| s.as_str()).collect();
                        let (subcategory, key) = match remaining.len() {
                            0 => return Err(VamoraError::new(1, "missing key argument")),
                            1 => (None, remaining[0].to_string()),
                            _ => (Some(remaining[0].to_string()), remaining[1].to_string()),
                        };
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                subcategory,
                                key: Some(key),
                                value: None,
                                system,
                            },
                        })
                    }

                    // put: vamorasys settings put <category> [subcategory] <key> <value>
                    // 2 remaining = key + value (no subcategory)
                    // 3 remaining = subcategory + key + value
                    SettingsAction::Put => {
                        let remaining: Vec<&str> = filtered[3..].iter().map(|s| s.as_str()).collect();
                        let (subcategory, key, value) = match remaining.len() {
                            0 | 1 => return Err(VamoraError::new(1,
                                "put requires: vamorasys settings put <category> [subcategory] <key> <value>")),
                            2 => (None, remaining[0].to_string(), remaining[1].to_string()),
                            _ => (Some(remaining[0].to_string()), remaining[1].to_string(), remaining[2].to_string()),
                        };
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                subcategory,
                                key: Some(key),
                                value: Some(value),
                                system,
                            },
                        })
                    }
                }
            }

            other => Err(VamoraError::new(1,
                format!("unknown command '{}'. Run 'vamorasys --help' for usage.", other))),
        }
    }
}