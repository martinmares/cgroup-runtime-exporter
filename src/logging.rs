#[macro_export]
macro_rules! log_anyhow_with_source {
    ($err:expr, $($rest:tt)+) => {{

        // Zajistíme, že to je něco, co implementuje std::error::Error
        let err: &anyhow::Error = &$err;

        if let Some(source) = err.source() {
            error!(
                error = %err,        // Display -> "read cpu.stat"
                caused_by = %source, // Display -> "No such file or directory (os error 2)"
                $($rest)+
            );
        } else {
            error!(
                error = %err,
                $($rest)+
            );
        }
    }};
}

#[macro_export]
macro_rules! log_promerror_with_source {
    ($err:expr, $($rest:tt)+) => {{
        // Zajistíme, že to je něco, co implementuje std::error::Error
        let err: &prometheus::Error = &$err;
          error!(
              error = %err,        // Display -> "read cpu.stat"
              $($rest)+
          );
        }};
}

#[macro_export]
macro_rules! log_hypererr_with_source {
    ($err:expr, $($rest:tt)+) => {{
        // Zajistíme, že to je něco, co implementuje std::error::Error
        let err: &hyper::Error = &$err;
          error!(
              error = %err,        // Display -> "read cpu.stat"
              $($rest)+
          );
        }};
}
