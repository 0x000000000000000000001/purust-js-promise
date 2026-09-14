pub fn Promise_Rejection_fromError(error: crate::UnknownType) -> crate::UnknownType {
    error
}

pub fn Promise_Rejection__toError() -> crate::UnknownType {
    purust_core::Value::Func3(purust_core::Func3::Static(|just, nothing, rejection| {
        // Match the native Error identity, not the shape of an arbitrary record.
        let is_error = match rejection.resolve() {
            purust_core::Value::Class(payload) =>
                payload.is::<std::sync::Arc<Purs_Effect_Exception::PurustExceptionError>>(),
            _ => false,
        };
        if is_error { just.unwrap_func1()(rejection) } else { nothing }
    }))
}
