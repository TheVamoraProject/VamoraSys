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
        category: String,
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
                let category = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys check <category>")
                })?;
                Ok(Cli { command: Command::Check { category: category.clone() } })
            }

            "exec" => {
                let name = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys exec <binary>")
                })?;
                Ok(Cli { command: Command::Exec { name: name.clone() } })
            }

            "utility" => {
                let name = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys utility <utility>")
                })?;
                Ok(Cli { command: Command::Utility { name: name.clone() } })
            }

            "vmf" => {
                // vamorasys vmf <file> <action> <key> [value]
                let file = args.get(1).ok_or_else(|| {
                    VamoraError::new(1, "usage: vamorasys vmf <file> <action> <key> [value]")
                })?;
                let action_str = args.get(2).ok_or_else(|| {
                    VamoraError::new(1, "missing action: put | get | delete | list")
                })?;

                let action = match action_str.as_str() {
                    "list" => {
                        // list doesn't need a key
                        return Ok(Cli {
                            command: Command::Vmf {
                                file: file.clone(),
                                action: VmfAction::List,
                                key: String::new(),
                                value: None,
                            },
                        });
                    }
                    "put" => VmfAction::Put,
                    "get" => VmfAction::Get,
                    "delete" => VmfAction::Delete,
                    other => return Err(VamoraError::new(1, format!("unknown vmf action '{}': use put | get | delete | list", other))),
                };

                let key = args.get(3).ok_or_else(|| {
                    VamoraError::new(1, "missing key argument")
                })?;
                let value = args.get(4).cloned();

                if matches!(action, VmfAction::Put) && value.is_none() {
                    return Err(VamoraError::new(1, "put requires a value: vamorasys vmf <file> put <key> <value>"));
                }

                Ok(Cli {
                    command: Command::Vmf {
                        file: file.clone(),
                        action,
                        key: key.clone(),
                        value,
                    },
                })
            }

            "settings" => {
                // vamorasys settings <action> <category> [key] [value] [--system]
                let system = args.contains(&"--system".to_string());
                let filtered: Vec<&String> = args.iter().filter(|a| a.as_str() != "--system").collect();

                let action_str = filtered.get(1).ok_or_else(|| {
                    VamoraError::new(1, "missing action: put | get | delete | list")
                })?;

                let action = match action_str.as_str() {
                    "put" => SettingsAction::Put,
                    "get" => SettingsAction::Get,
                    "delete" => SettingsAction::Delete,
                    "list" => SettingsAction::List,
                    other => return Err(VamoraError::new(1, format!("unknown action '{}': use put | get | delete | list", other))),
                };

                let category = filtered.get(2).ok_or_else(|| {
                    VamoraError::new(1, "missing category argument")
                })?.to_string();

                match action {
                    SettingsAction::List => {
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                key: None,
                                value: None,
                                system,
                            },
                        })
                    }
                    SettingsAction::Get | SettingsAction::Delete => {
                        let key = filtered.get(3).ok_or_else(|| {
                            VamoraError::new(1, "missing key argument")
                        })?.to_string();
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                key: Some(key),
                                value: None,
                                system,
                            },
                        })
                    }
                    SettingsAction::Put => {
                        let key = filtered.get(3).ok_or_else(|| {
                            VamoraError::new(1, "missing key argument")
                        })?.to_string();
                        let value = filtered.get(4).ok_or_else(|| {
                            VamoraError::new(1, "missing value argument")
                        })?.to_string();
                        Ok(Cli {
                            command: Command::Settings {
                                action,
                                category,
                                key: Some(key),
                                value: Some(value),
                                system,
                            },
                        })
                    }
                }
            }

            other => Err(VamoraError::new(1, format!("unknown command '{}'. Run 'vamorasys --help' for usage.", other))),
        }
    }
}
