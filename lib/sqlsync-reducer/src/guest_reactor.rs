use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{Mutex, OnceLock},
    task::{Context, Poll},
};

use serde::de::DeserializeOwned;

use crate::{
    guest_ffi::{fbm, FFIBufPtr},
    types::{
        ErrorResponse, ExecResponse, QueryResponse, ReducerError, Request, RequestId, Requests,
        Responses, SqliteValue,
    },
};

pub fn reactor() -> std::sync::MutexGuard<'static, Reactor> {
    static SINGLETON: OnceLock<Mutex<Reactor>> = OnceLock::new();
    let m = SINGLETON.get_or_init(|| Mutex::new(Reactor::new()));
    m.lock().unwrap()
}

type ReducerTask = Pin<Box<dyn Future<Output = Result<(), ReducerError>> + Send + 'static>>;

#[derive(Default)]
pub struct Reactor {
    task: Option<ReducerTask>,
    request_id_generator: RequestId,

    // requests from guest -> host
    requests: Requests,
    // responses from host -> guest
    responses: Responses,
}

impl Reactor {
    pub fn new() -> Self {
        Reactor::default()
    }

    fn queue_request(&mut self, request: Request) -> RequestId {
        let id = self.request_id_generator;
        self.request_id_generator = self.request_id_generator.wrapping_add(1);
        self.requests
            .get_or_insert_with(BTreeMap::new)
            .insert(id, request);
        id
    }

    fn get_response<T: DeserializeOwned>(&mut self, id: RequestId) -> Option<T> {
        self.responses
            .as_mut()
            .and_then(|b| b.remove(&id))
            .map(|ptr| {
                let mut f = fbm();
                unsafe { f.decode(ptr).unwrap() }
            })
    }

    pub fn spawn(&mut self, task: ReducerTask) {
        if self.task.is_some() {
            panic!("Reducer task already running");
        }
        self.task = Some(task);
    }

    pub fn step(&mut self, responses: Responses) -> Result<Requests, ReducerError> {
        if let Some(ref mut previous) = self.responses {
            // if we still have previous responses, merge new responses in
            // this replaces keys in previous with those in next - as long
            // as the host respects the request indexes this is safe
            if let Some(mut next) = responses {
                previous.append(&mut next);
            }
        } else {
            // otherwise, just use the new responses
            self.responses = responses;
        }

        if let Some(mut task) = self.task.take() {
            let mut ctx = Context::from_waker(futures::task::noop_waker_ref());
            match task.as_mut().poll(&mut ctx) {
                Poll::Ready(result) => result?,
                Poll::Pending => {
                    self.task = Some(task);
                }
            }
        }

        Ok(self.requests.take())
    }
}

#[must_use]
pub struct ResponseFuture<T: DeserializeOwned> {
    id: RequestId,
    _marker: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned> ResponseFuture<T> {
    fn new(id: RequestId) -> Self {
        Self { id, _marker: std::marker::PhantomData }
    }
}

impl<T: DeserializeOwned> Future for ResponseFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let mut r = reactor();
        match r.get_response(self.id) {
            Some(response) => Poll::Ready(response),
            None => Poll::Pending,
        }
    }
}

pub fn raw_query(
    sql: String,
    params: Vec<SqliteValue>,
) -> ResponseFuture<Result<QueryResponse, ErrorResponse>> {
    let request = Request::Query { sql, params };
    let mut r = reactor();
    let id = r.queue_request(request);
    ResponseFuture::new(id)
}

pub fn raw_execute(
    sql: String,
    params: Vec<SqliteValue>,
) -> ResponseFuture<Result<ExecResponse, ErrorResponse>> {
    let request = Request::Exec { sql, params };
    let mut r = reactor();
    let id = r.queue_request(request);
    ResponseFuture::new(id)
}

#[macro_export]
macro_rules! query {
    ($sql:expr $(, $arg:expr)*) => {
        sqlsync_reducer::guest_reactor::raw_query($sql.into(), vec![$($arg.into()),*])
    };
}

#[macro_export]
macro_rules! execute {
    ($sql:expr $(, $arg:expr)*) => {
        sqlsync_reducer::guest_reactor::raw_execute($sql.into(), vec![$($arg.into()),*])
    };
}

#[macro_export]
macro_rules! init_reducer {
    // fn should be (Vec<u8>) -> Future<Output = Result<(), ReducerError>>
    ($fn:ident) => {
        /// ffi_reduce is called by the host to cause the reducer to start processing a new mutation.
        ///
        /// # Panics
        /// Panics if the host passes in an invalid pointer.
        /// # Safety
        /// The host must pass in a valid pointer to a Mutation buffer.
        #[no_mangle]
        pub unsafe fn ffi_reduce(
            mutation_ptr: sqlsync_reducer::guest_ffi::FFIBufPtr,
        ) -> sqlsync_reducer::guest_ffi::FFIBufPtr {
            let mut reactor = sqlsync_reducer::guest_reactor::reactor();
            let mut fbm = sqlsync_reducer::guest_ffi::fbm();
            let mutation = fbm.consume(mutation_ptr);

            reactor.spawn(Box::pin(async move { $fn(mutation).await }));

            let requests = reactor.step(None);
            fbm.encode(&requests).unwrap()
        }

        static LOGGER: sqlsync_reducer::guest_ffi::FFILogger =
            sqlsync_reducer::guest_ffi::FFILogger;

        #[no_mangle]
        pub extern "C" fn ffi_init_reducer() {
            LOGGER.init(log::Level::Trace).unwrap();
            sqlsync_reducer::guest_ffi::install_panic_hook();
        }
    };
}

/// ffi_reactor_step is called by the host to advance the reactor forward.
///
/// # Panics
/// Panics if the host passes in an invalid pointer.
///
/// # Safety
/// The host must pass in a valid pointer to a serialized Responses object.
#[no_mangle]
pub unsafe fn ffi_reactor_step(responses_ptr: FFIBufPtr) -> FFIBufPtr {
    let mut fbm = fbm();
    let responses = fbm.decode(responses_ptr).unwrap();
    let mut r = reactor();
    let out = r.step(responses);
    fbm.encode(&out).unwrap()
}
