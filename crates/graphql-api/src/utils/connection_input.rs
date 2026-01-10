use crate::cursors::ConnectionParameters;

pub(crate) struct ConnectionInputBuilder<C, F, S> {
    connection_parameters: ConnectionParameters<C>,
    filter: Option<F>,
    sort: Option<S>,
}

// derive(Default) adds Default: bound to the generics
impl<C, F, S> Default for ConnectionInputBuilder<C, F, S> {
    fn default() -> Self {
        Self {
            connection_parameters: Default::default(),
            filter: Default::default(),
            sort: Default::default(),
        }
    }
}

impl<C, F, S> ConnectionInputBuilder<C, F, S> {
    pub(crate) fn new(connection_parameters: ConnectionParameters<C>) -> Self {
        Self {
            connection_parameters,
            ..Default::default()
        }
    }

    pub(crate) fn with_filter(mut self, filter: Option<F>) -> Self {
        self.filter = filter;
        self
    }

    pub(crate) fn with_sort(mut self, sort: Option<S>) -> Self {
        self.sort = sort;
        self
    }

    pub(crate) fn build<Src>(self, source: Src) -> ConnectionInput<C, F, S, Src> {
        ConnectionInput {
            connection_parameters: self.connection_parameters,
            filter: self.filter,
            sort: self.sort,
            source,
        }
    }
}

pub(crate) struct ConnectionInput<C, F, S, Src> {
    pub(crate) connection_parameters: ConnectionParameters<C>,
    pub(crate) filter: Option<F>,
    pub(crate) sort: Option<S>,
    pub(crate) source: Src,
}
