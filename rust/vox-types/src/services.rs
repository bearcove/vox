use facet::{Facet, Shape};

/// Static descriptor for a vox RPC service.
///
/// Contains the service name and all method descriptors. Built once per service
/// via OnceLock in macro-generated code.
pub struct ServiceDescriptor {
    /// Service name (e.g., "Calculator").
    pub service_name: &'static str,

    /// All methods in this service.
    pub methods: &'static [&'static MethodDescriptor],

    /// Documentation string, if any.
    pub doc: Option<&'static str>,
}

impl ServiceDescriptor {
    /// Look up a method descriptor by method ID.
    pub fn by_id(&self, method_id: MethodId) -> Option<&'static MethodDescriptor> {
        self.methods.iter().find(|m| m.id == method_id).copied()
    }
}

/// Static descriptor for a single RPC method.
///
/// Contains static metadata needed for dispatching and calling this method.
pub struct MethodDescriptor {
    /// Method ID (hash of service name, method name, arg shapes, return shape).
    pub id: MethodId,

    /// Service name (e.g., "Calculator").
    pub service_name: &'static str,

    /// Method name (e.g., "add").
    pub method_name: &'static str,

    /// Args type shape
    pub args_shape: &'static Shape,

    /// Arguments in declaration order.
    pub args: &'static [ArgDescriptor],

    /// Return type shape.
    pub return_shape: &'static Shape,

    /// Whether `args_shape` reaches a channel (Tx/Rx) anywhere in its tree.
    /// Computed once via `shape_contains_channel` when the descriptor is
    /// built, so the driver's cancel/failure paths can short-circuit
    /// instead of re-walking the shape per request.
    pub args_have_channels: bool,

    /// Documentation string, if any.
    pub doc: Option<&'static str>,
}

impl std::fmt::Debug for MethodDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MethodDescriptor")
            .field("id", &self.id)
            .field("service_name", &self.service_name)
            .field("method_name", &self.method_name)
            .finish_non_exhaustive()
    }
}

declare_id!(
    /// A unique method identifier — hash of service name, method name, arg shapes, return shape
    MethodId, u64
);

/// Descriptor for a single RPC method argument.
///
/// Contains metadata about an argument including its name, shape, and
/// whether it's a channel type (Rx/Tx).
#[derive(Debug)]
pub struct ArgDescriptor {
    /// Argument name (e.g., "user_id", "stream").
    pub name: &'static str,

    /// Argument type shape.
    pub shape: &'static Shape,

    /// For a `Tx<T>`/`Rx<T>` argument, the element type's shape (`T::SHAPE`).
    ///
    /// `Tx`/`Rx` are `#[facet(opaque)]`, so their `Shape` carries no
    /// `type_params` — the element type is invisible to reflection. The service
    /// macro, which sees the channel type syntactically, captures it here so
    /// codegen can emit the per-channel element schema.
    pub channel_element: Option<&'static Shape>,
}

impl ServiceDescriptor {
    /// An empty service descriptor for dispatchers that don't serve any methods.
    pub const EMPTY: ServiceDescriptor = ServiceDescriptor {
        service_name: "<Empty>",
        methods: &[],
        doc: None,
    };
}
