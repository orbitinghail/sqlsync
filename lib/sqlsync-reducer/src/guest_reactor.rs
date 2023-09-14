use std::{
    collections::BTreeMap,
    future::Future,
    mem::MaybeUninit,
    pin::Pin,
    sync::Once,
    task::{Context, Poll},
};

use serde::de::DeserializeOwned;

use crate::{
    guest_ffi::{fbm, FFIBufPtr},
    types::{
        ExecResponse, QueryResponse, ReducerError, Request, RequestId, Requests, Responses,
        SqliteValue,
    },
};

pub fn reactor() -> &'static mut Reactor {
    static mut SINGLETON: MaybeUninit<Reactor> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();
    unsafe {
        ONCE.call_once(|| {
            let singleton = Reactor::new();
            SINGLETON.write(singleton);
        });
        SINGLETON.assume_init_mut()
    }
}

type ReducerTask = Pin<Box<dyn Future<Output = Result<(), ReducerError>>>>;

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
        Self { task: None, request_id_generator: 0, requests: None, responses: None }
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
            .map(|ptr| fbm().decode(ptr as *mut u8).unwrap())
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
        match reactor().get_response(self.id) {
            Some(response) => Poll::Ready(response),
            None => Poll::Pending,
        }
    }
}

pub fn raw_query(sql: String, params: Vec<SqliteValue>) -> ResponseFuture<QueryResponse> {
    let request = Request::Query { sql, params };
    let id = reactor().queue_request(request);
    ResponseFuture::new(id)
}

pub fn raw_execute(sql: String, params: Vec<SqliteValue>) -> ResponseFuture<ExecResponse> {
    let request = Request::Exec { sql, params };
    let id = reactor().queue_request(request);
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
        #[no_mangle]
        pub fn ffi_reduce(
            mutation_ptr: sqlsync_reducer::guest_ffi::FFIBufPtr,
        ) -> sqlsync_reducer::guest_ffi::FFIBufPtr {
            let reactor = sqlsync_reducer::guest_reactor::reactor();
            let fbm = sqlsync_reducer::guest_ffi::fbm();
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

#[no_mangle]
pub fn ffi_reactor_step(responses_ptr: FFIBufPtr) -> FFIBufPtr {
    let fbm = fbm();
    let responses = fbm.decode(responses_ptr).unwrap();
    let out = reactor().step(responses);
    fbm.encode(&out).unwrap()
}
