#[macro_export]
macro_rules! log_anyhow_with_source {
    ($err:expr, $($rest:tt)+) => {{
        // Jasně řekneme, že pracujeme s anyhow::Error
        let err: &anyhow::Error = &$err;

        // Nejnižší příčina chyby (root cause)
        let root = err.root_cause();

        ::tracing::error!(
            error = %err,       // např. "read cpu.stat"
            root_cause = %root, // např. "No such file or directory (os error 2)"
            $($rest)+
        );
    }};
}

#[macro_export]
macro_rules! log_error_display {
    ($err:expr, $($rest:tt)+) => {{
        ::tracing::error!(
            error = %$err,
            $($rest)+
        );
    }};
}
