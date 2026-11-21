//! Puts the GraphQL path being resolved in the context of every log line.
//!
//! Each field resolution runs inside a `gql` span holding its path, so a log emitted while
//! resolving `rank` of a `record` reads like this:
//!
//! ```text
//! INFO HTTP request{http.method=POST http.route=/graphql ...}:gql{path=record.rank}: ...
//! ```
//!
//! The spans aren't nested in each other, but all in the span that was current when the request
//! started (the HTTP request span): the path already says where the field is, and nesting them would
//! repeat it for each level.
//!
//! When the [`TARGET`] is enabled at the `DEBUG` level, each segment of the path also shows the
//! arguments given to its field, with the variables replaced by their values:
//!
//! ```text
//! gql{path=records(first: 10)[3].map.name}
//! ```
//!
//! There is one span per resolved field, so a subscriber which logs span closes (like
//! `FmtSpan::CLOSE`) would log a line for each one of them. Filter out span events with the
//! [`TARGET`] target if that's not wanted.

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Arc, Mutex, OnceLock},
};

use async_graphql::{
    Response, ServerResult, Value, Variables,
    extensions::{
        Extension, ExtensionContext, ExtensionFactory, NextParseQuery, NextRequest, NextResolve,
        NextSubscribe, ResolveInfo,
    },
    futures_util::stream::BoxStream,
    parser::types::ExecutableDocument,
};
use tracing::{Instrument as _, Level, Span, field::Empty};

/// The target of the spans created by [`PathTracing`].
pub const TARGET: &str = "graphql_api::path";

/// The extension which creates the `gql` spans. See the [module documentation](self).
pub struct PathTracing;

impl ExtensionFactory for PathTracing {
    fn create(&self) -> Arc<dyn Extension> {
        let with_args = tracing::enabled!(target: TARGET, Level::DEBUG);
        Arc::new(PathTracingExtension {
            root: OnceLock::new(),
            args: with_args.then(Default::default),
        })
    }
}

/// What's needed to show the arguments in the path.
#[derive(Default)]
struct ArgsState {
    /// The variables of the request, to show their values instead of their names.
    variables: OnceLock<Variables>,
    /// The full path (with arguments) of each field being resolved, by its span.
    ///
    /// The path of a field only knows the arguments of the field itself, so its children look up
    /// the path of their parent here.
    paths: Mutex<HashMap<tracing::Id, Arc<str>>>,
}

/// One per request.
struct PathTracingExtension {
    /// The span the `gql` spans are created in.
    root: OnceLock<Span>,
    /// `None` when the arguments aren't shown.
    args: Option<ArgsState>,
}

#[async_graphql::async_trait::async_trait]
impl Extension for PathTracingExtension {
    async fn request(&self, ctx: &ExtensionContext<'_>, next: NextRequest<'_>) -> Response {
        let _ = self.root.set(Span::current());
        next.run(ctx).await
    }

    fn subscribe<'s>(
        &self,
        ctx: &ExtensionContext<'_>,
        stream: BoxStream<'s, Response>,
        next: NextSubscribe<'_>,
    ) -> BoxStream<'s, Response> {
        let _ = self.root.set(Span::current());
        next.run(ctx, stream)
    }

    async fn parse_query(
        &self,
        ctx: &ExtensionContext<'_>,
        query: &str,
        variables: &Variables,
        next: NextParseQuery<'_>,
    ) -> ServerResult<ExecutableDocument> {
        if let Some(args) = &self.args {
            let _ = args.variables.set(variables.clone());
        }
        next.run(ctx, query, variables).await
    }

    async fn resolve(
        &self,
        ctx: &ExtensionContext<'_>,
        info: ResolveInfo<'_>,
        next: NextResolve<'_>,
    ) -> ServerResult<Option<Value>> {
        // The items of a list are resolved too, but their index already shows in the path of
        // their fields.
        if info.is_for_introspection
            || matches!(
                info.path_node.segment,
                async_graphql::QueryPathSegment::Index(_)
            )
        {
            return next.run(ctx, info).await;
        }

        let root = self.root.get().cloned().unwrap_or_else(Span::current);
        let span = tracing::info_span!(target: TARGET, parent: &root, "gql", path = Empty);
        let Some(id) = span.id() else {
            return next.run(ctx, info).await;
        };

        let Some(args) = &self.args else {
            let mut path = String::new();
            write_plain_path(&mut path, info.path_node);
            span.record("path", path.as_str());
            return next.run(ctx, info).instrument(span).await;
        };

        // The parent field is being resolved in the current span.
        let parent_path = Span::current()
            .id()
            .and_then(|id| args.paths.lock().unwrap().get(&id).cloned());
        let path: Arc<str> = path_with_args(&info, parent_path.as_deref(), args).into();
        span.record("path", &*path);

        args.paths.lock().unwrap().insert(id.clone(), path);
        let _guard = RemovePathOnDrop(&args.paths, id);
        next.run(ctx, info).instrument(span).await
    }
}

/// Removes the path of a field when it's resolved, or when the resolution is cancelled.
struct RemovePathOnDrop<'a>(&'a Mutex<HashMap<tracing::Id, Arc<str>>>, tracing::Id);

impl Drop for RemovePathOnDrop<'_> {
    fn drop(&mut self) {
        if let Ok(mut paths) = self.0.lock() {
            paths.remove(&self.1);
        }
    }
}

/// Writes the path like `records[3].map.name`.
fn write_plain_path(out: &mut String, node: &async_graphql::QueryPathNode<'_>) {
    if let Some(parent) = node.parent {
        write_plain_path(out, parent);
    }
    match node.segment {
        async_graphql::QueryPathSegment::Index(idx) => {
            let _ = write!(out, "[{idx}]");
        }
        async_graphql::QueryPathSegment::Name(name) => {
            if node.parent.is_some() {
                out.push('.');
            }
            out.push_str(name);
        }
    }
}

/// Builds the path of the field with its arguments, like `records(first: 10)[3].map`.
fn path_with_args(info: &ResolveInfo<'_>, parent_path: Option<&str>, args: &ArgsState) -> String {
    // The path node of the field is preceded by the indexes of the parent lists, if any, then by
    // the path node of the parent field.
    let mut indexes = Vec::new();
    let mut parent_field = info.path_node.parent;
    while let Some(node) = parent_field
        && let async_graphql::QueryPathSegment::Index(idx) = node.segment
    {
        indexes.push(idx);
        parent_field = node.parent;
    }

    let mut out = String::new();
    match (parent_path, parent_field) {
        (Some(parent_path), _) => out.push_str(parent_path),
        // Shouldn't happen, but at least show the path without the arguments.
        (None, Some(parent_field)) => write_plain_path(&mut out, parent_field),
        (None, None) => (),
    }
    for idx in indexes.iter().rev() {
        let _ = write!(out, "[{idx}]");
    }
    if !out.is_empty() {
        out.push('.');
    }
    if let async_graphql::QueryPathSegment::Name(name) = info.path_node.segment {
        out.push_str(name);
    }

    let field_args = &info.field.arguments;
    if !field_args.is_empty() {
        out.push('(');
        for (i, (name, value)) in field_args.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            let value = value.node.clone();
            let _ = match args.variables.get() {
                Some(vars) => match value
                    .clone()
                    .into_const_with(|var| vars.get(&var).cloned().ok_or(()))
                {
                    Ok(value) => write!(out, "{}: {value}", name.node),
                    Err(()) => write!(out, "{}: {value}", name.node),
                },
                None => write!(out, "{}: {value}", name.node),
            };
        }
        out.push(')');
    }

    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_graphql::{EmptyMutation, EmptySubscription, Object, Request, Schema, Variables};
    use tracing::{
        Level, Subscriber,
        field::{Field, Visit},
        span::Attributes,
    };
    use tracing_subscriber::{
        Layer, filter::Targets, layer::Context, layer::SubscriberExt as _, registry::LookupSpan,
    };

    use super::{PathTracing, TARGET};

    struct Query;

    #[Object]
    impl Query {
        async fn record(&self, #[graphql(name = "login")] _login: String) -> Record {
            Record
        }

        async fn records(&self, first: i32) -> Vec<Record> {
            vec![Record; first as usize]
        }
    }

    #[derive(Clone)]
    struct Record;

    #[Object]
    impl Record {
        async fn rank(&self) -> i32 {
            // Stands for a log emitted while resolving the field.
            tracing::info!("resolving rank");
            1
        }
    }

    /// Collects the paths of the `gql` spans, and the paths in which the events are emitted.
    #[derive(Clone, Default)]
    struct Collect {
        spans: Arc<Mutex<Vec<String>>>,
        events: Arc<Mutex<Vec<String>>>,
    }

    struct PathField(String);

    impl Visit for PathField {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "path" {
                self.0 = value.to_owned();
            }
        }

        fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
    }

    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Collect {
        fn on_new_span(&self, _: &Attributes<'_>, _: &tracing::Id, _: Context<'_, S>) {}

        fn on_record(
            &self,
            _: &tracing::Id,
            values: &tracing::span::Record<'_>,
            _: Context<'_, S>,
        ) {
            let mut path = PathField(String::new());
            values.record(&mut path);
            self.spans.lock().unwrap().push(path.0);
        }

        fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
            let scope: Vec<_> = ctx
                .event_scope(event)
                .into_iter()
                .flatten()
                .map(|span| span.name())
                .collect();
            self.events.lock().unwrap().push(scope.join(":"));
        }
    }

    async fn run(level: Level, query: &str, variables: serde_json::Value) -> Collect {
        let collect = Collect::default();
        let subscriber = tracing_subscriber::registry().with(collect.clone()).with(
            Targets::new()
                .with_target(TARGET, level)
                .with_default(Level::INFO),
        );
        let _guard = tracing::subscriber::set_default(subscriber);

        let schema = Schema::build(Query, EmptyMutation, EmptySubscription)
            .extension(PathTracing)
            .finish();
        let root = tracing::info_span!("HTTP request");
        let res = async {
            schema
                .execute(Request::new(query).variables(Variables::from_json(variables)))
                .await
        }
        .instrument_in(root)
        .await;
        assert!(res.errors.is_empty(), "{:?}", res.errors);

        collect
    }

    trait InstrumentIn: std::future::Future + Sized {
        fn instrument_in(self, span: tracing::Span) -> tracing::instrument::Instrumented<Self> {
            tracing::Instrument::instrument(self, span)
        }
    }

    impl<F: std::future::Future> InstrumentIn for F {}

    #[tokio::test]
    async fn plain_paths_at_info() {
        let collect = run(
            Level::INFO,
            r#"{ record(login: "foo") { rank } records(first: 2) { r: rank } }"#,
            serde_json::json!({}),
        )
        .await;

        let mut spans = collect.spans.lock().unwrap().clone();
        spans.sort();
        assert_eq!(
            spans,
            [
                "record",
                "record.rank",
                "records",
                "records[0].r",
                "records[1].r"
            ]
        );

        // Not nested in each other, but all in the request span.
        let events = collect.events.lock().unwrap().clone();
        assert_eq!(events, ["gql:HTTP request"; 3]);
    }

    #[tokio::test]
    async fn paths_with_args_at_debug() {
        let collect = run(
            Level::DEBUG,
            r#"query($login: String!) { record(login: $login) { rank } records(first: 1) { rank } }"#,
            serde_json::json!({ "login": "foo" }),
        )
        .await;

        let mut spans = collect.spans.lock().unwrap().clone();
        spans.sort();
        assert_eq!(
            spans,
            [
                r#"record(login: "foo")"#,
                r#"record(login: "foo").rank"#,
                "records(first: 1)",
                "records(first: 1)[0].rank",
            ]
        );
    }
}
