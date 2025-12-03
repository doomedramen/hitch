//! Convenience macros for logging that maintain backward compatibility
//! while supporting structured logging when enabled.

/// Macro for debug logging that works with both simple and structured logging
#[macro_export]
macro_rules! log_debug {
    ($context:expr, $($arg:tt)*) => {{
        if $context.verbose {
            $context.logger.debug(&format!($($arg)*));
            // Also print to stdout for backward compatibility
            println!("{}", format!($($arg)*));
        }
    }};
}

/// Macro for info logging that works with both simple and structured logging
#[macro_export]
macro_rules! log_info {
    ($context:expr, $($arg:tt)*) => {{
        $context.logger.info(&format!($($arg)*));
        // Also print to stdout with info emoji for backward compatibility
        println!("{} {}", "ℹ️".blue(), format!($($arg)*));
    }};
}

/// Macro for warning logging that works with both simple and structured logging
#[macro_export]
macro_rules! log_warning {
    ($context:expr, $($arg:tt)*) => {{
        $context.logger.warn(&format!($($arg)*));
        // Also print to stdout with warning emoji for backward compatibility
        println!("{} {}", "⚠️".yellow(), format!($($arg)*));
    }};
}

/// Macro for error logging that works with both simple and structured logging
#[macro_export]
macro_rules! log_error {
    ($context:expr, $($arg:tt)*) => {{
        $context.logger.error(&format!($($arg)*));
        // Also print to stderr with error emoji for backward compatibility
        eprintln!("{} {}", "❌".red(), format!($($arg)*));
    }};
}

/// Macro for debug logging with structured fields
#[macro_export]
macro_rules! log_debug_with {
    ($context:expr, $message:expr, $($key:ident = $value:expr),* $(,)?) => {{
        if $context.verbose {
            $context.logger.debug_with($message, |entry| {
                $(
                    entry.field(stringify!($key), $value);
                )*
                entry
            });
            // Also print to stdout for backward compatibility
            let fields = vec![$(format!("{}={}", stringify!($key), $value)),*];
            if !fields.is_empty() {
                println!("{} {}", $message, fields.join(" "));
            } else {
                println!("{}", $message);
            }
        }
    }};
}

/// Macro for info logging with structured fields
#[macro_export]
macro_rules! log_info_with {
    ($context:expr, $message:expr, $($key:ident = $value:expr),* $(,)?) => {{
        $context.logger.info_with($message, |entry| {
            $(
                entry.field(stringify!($key), $value);
            )*
            entry
        });
        // Also print to stdout with info emoji for backward compatibility
        let fields = vec![$(format!("{}={}", stringify!($key), $value)),*];
        if !fields.is_empty() {
            println!("{} {} {}", "ℹ️".blue(), $message, fields.join(" "));
        } else {
            println!("{} {}", "ℹ️".blue(), $message);
        }
    }};
}
