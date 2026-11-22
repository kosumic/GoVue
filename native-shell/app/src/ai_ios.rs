use std::{
    ffi::c_void,
    ptr,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use block2::Block;
use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use objc2::{
    AnyThread, DefinedClass, define_class, msg_send,
    rc::{Retained, autoreleasepool},
    runtime::{AnyClass, AnyObject, Bool},
};
use objc2_foundation::{
    NSData, NSError, NSObject, NSObjectProtocol, NSString, NSURL, NSURLRequest,
};
use url::Url;

const SERVICE: &str = "io.github.kaminarios.lingoplay.ai";
const NOT_FOUND: i32 = -25300;

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: *const AnyObject;
    static kSecClassGenericPassword: *const AnyObject;
    static kSecAttrService: *const AnyObject;
    static kSecAttrAccount: *const AnyObject;
    static kSecReturnData: *const AnyObject;
    static kSecMatchLimit: *const AnyObject;
    static kSecMatchLimitOne: *const AnyObject;
    static kSecValueData: *const AnyObject;
    static kSecAttrAccessible: *const AnyObject;
    static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: *const AnyObject;

    fn SecItemCopyMatching(query: *const c_void, result: *mut *mut c_void) -> i32;
    fn SecItemUpdate(query: *const c_void, attributes: *const c_void) -> i32;
    fn SecItemAdd(query: *const c_void, result: *mut *mut c_void) -> i32;
    fn SecItemDelete(query: *const c_void) -> i32;
}

pub fn install() {
    govue_core::install_ai_platform(Box::new(IosAi));
}

struct IosAi;

impl AiPlatform for IosAi {
    fn secret_read(&self, key: String) -> AiSecret {
        autoreleasepool(|_| {
            if !valid_key(&key) {
                return secret(None);
            }
            let query = query(&key);
            // SAFETY: Query and keys are toll-free bridged Foundation and Security
            // objects. A successful copy with kSecReturnData owns one CFData.
            unsafe {
                let yes: Retained<AnyObject> =
                    msg_send![class(c"NSNumber"), numberWithBool: Bool::YES];
                let _: () = msg_send![&*query, setObject: &*yes, forKey: kSecReturnData];
                let _: () =
                    msg_send![&*query, setObject: kSecMatchLimitOne, forKey: kSecMatchLimit];
                let mut data = ptr::null_mut();
                match SecItemCopyMatching(dictionary(&query), &mut data) {
                    0 => secret(
                        Retained::from_raw(data.cast::<NSData>())
                            .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok()),
                    ),
                    NOT_FOUND => secret(Some(String::new())),
                    _ => secret(None),
                }
            }
        })
    }

    fn secret_write(&self, key: String, value: String) -> bool {
        autoreleasepool(|_| {
            if !valid_key(&key) || value.len() > 100_000 {
                return false;
            }
            let query = query(&key);
            // SAFETY: Security takes only the two retained Foundation dictionaries.
            // Every attribute key/value supplied from Security is an immortal CFString.
            unsafe {
                if value.is_empty() {
                    return matches!(SecItemDelete(dictionary(&query)), 0 | NOT_FOUND);
                }
                let data = NSData::with_bytes(value.as_bytes());
                let attributes: Retained<AnyObject> = msg_send![class(c"NSMutableDictionary"), new];
                let _: () = msg_send![&*attributes, setObject: &*data, forKey: kSecValueData];
                let _: () = msg_send![
                    &*attributes, setObject: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
                    forKey: kSecAttrAccessible
                ];
                match SecItemUpdate(dictionary(&query), dictionary(&attributes)) {
                    0 => true,
                    NOT_FOUND => {
                        let _: () = msg_send![&*query, addEntriesFromDictionary: &*attributes];
                        SecItemAdd(dictionary(&query), ptr::null_mut()) == 0
                    }
                    _ => false,
                }
            }
        })
    }

    fn request(&self, request: AiHttpRequest, response: Arc<AiResponse>) {
        autoreleasepool(|_| {
            if response.is_cancelled() {
                return;
            }
            if !provider_url(&request.url) || !matches!(request.method.as_str(), "GET" | "POST") {
                response.failed();
                return;
            }
            let Some(url) = NSURL::URLWithString(&NSString::from_str(&request.url)) else {
                response.failed();
                return;
            };
            let (done, receiver) = mpsc::channel();
            let delegate = Transport::alloc().set_ivars(Transfer {
                response: response.clone(),
                done,
            });
            // SAFETY: Constructs one ephemeral URLSession with the delegate and
            // request below. URLSession owns the delegate until invalidation.
            let (session, task) = unsafe {
                let delegate: Retained<Transport> = msg_send![super(delegate), init];
                let native: Retained<AnyObject> =
                    msg_send![class(c"NSMutableURLRequest"), requestWithURL: &*url];
                let _: () =
                    msg_send![&*native, setHTTPMethod: &*NSString::from_str(&request.method)];
                if request.method == "POST" {
                    let body = NSData::with_bytes(request.body.as_bytes());
                    let _: () = msg_send![&*native, setHTTPBody: &*body];
                }
                for header in request.headers {
                    let name = NSString::from_str(&header.name);
                    let value = NSString::from_str(&header.value);
                    let _: () = msg_send![&*native, setValue: &*value, forHTTPHeaderField: &*name];
                }
                let options: Retained<AnyObject> = msg_send![
                    class(c"NSURLSessionConfiguration"),
                    ephemeralSessionConfiguration
                ];
                let nil = ptr::null::<AnyObject>();
                let _: () = msg_send![&*options, setHTTPCookieStorage: nil];
                let _: () = msg_send![&*options, setURLCache: nil];
                let _: () = msg_send![&*options, setTimeoutIntervalForRequest: 600.0_f64];
                let _: () = msg_send![&*options, setTimeoutIntervalForResource: 900.0_f64];
                let session: Retained<AnyObject> = msg_send![
                    class(c"NSURLSession"), sessionWithConfiguration: &*options,
                    delegate: &*delegate, delegateQueue: nil
                ];
                let task: Retained<AnyObject> = msg_send![&*session, dataTaskWithRequest: &*native];
                let _: () = msg_send![&*task, resume];
                (session, task)
            };
            let deadline = Instant::now() + Duration::from_secs(905);
            while let Err(mpsc::RecvTimeoutError::Timeout) =
                receiver.recv_timeout(Duration::from_millis(200))
            {
                if response.is_cancelled() {
                    break;
                }
                if Instant::now() >= deadline {
                    response.failed();
                    break;
                }
            }
            // SAFETY: Both are the live URLSession objects returned above.
            unsafe {
                let _: () = msg_send![&*task, cancel];
                let _: () = msg_send![&*session, invalidateAndCancel];
            }
        });
    }
}

struct Transfer {
    response: Arc<AiResponse>,
    done: mpsc::Sender<()>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = AnyThread]
    #[ivars = Transfer]
    struct Transport;

    unsafe impl NSObjectProtocol for Transport {}

    impl Transport {
        #[unsafe(method(URLSession:task:willPerformHTTPRedirection:newRequest:completionHandler:))]
        fn redirect(&self, _: &AnyObject, _: &AnyObject, _: &AnyObject, _: &NSURLRequest,
            continuation: &Block<dyn Fn(*const NSURLRequest)>) {
            continuation.call((ptr::null(),));
        }

        #[unsafe(method(URLSession:dataTask:didReceiveResponse:completionHandler:))]
        fn headers(&self, _: &AnyObject, _: &AnyObject, reply: &AnyObject,
            continuation: &Block<dyn Fn(isize)>) {
            // SAFETY: NSURLSession supplies NSURLResponse, a subclass that is
            // queried for statusCode only if it is an NSHTTPURLResponse.
            let status = unsafe {
                let http: Bool = msg_send![reply, isKindOfClass: class(c"NSHTTPURLResponse")];
                http.as_bool().then(|| { let status: isize = msg_send![reply, statusCode]; status })
            };
            if let Some(status) = status.and_then(|number| i32::try_from(number).ok()) {
                self.ivars().response.start(status);
                continuation.call((1,));
            } else {
                self.ivars().response.failed();
                continuation.call((0,));
            }
        }

        #[unsafe(method(URLSession:dataTask:didReceiveData:))]
        fn chunk(&self, _: &AnyObject, task: &AnyObject, data: &NSData) {
            if !self.ivars().response.push(data.to_vec()) {
                // SAFETY: The delegate receives its active NSURLSessionDataTask.
                let _: () = unsafe { msg_send![task, cancel] };
            }
        }

        #[unsafe(method(URLSession:task:didCompleteWithError:))]
        fn complete(&self, _: &AnyObject, _: &AnyObject, error: Option<&NSError>) {
            if error.is_some_and(|error| error.code() != -999) {
                self.ivars().response.failed();
            }
            let _ = self.ivars().done.send(());
        }
    }
);

fn valid_key(key: &str) -> bool {
    key.starts_with("ai.")
        && key.len() <= 100
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
}

fn provider_url(address: &str) -> bool {
    Url::parse(address).is_ok_and(|url| {
        url.username().is_empty()
            && url.password().is_none()
            && (url.scheme() == "https"
                || (url.scheme() == "http"
                    && matches!(url.host_str(), Some("127.0.0.1" | "localhost"))))
    })
}

fn class(name: &std::ffi::CStr) -> &'static AnyClass {
    AnyClass::get(name).expect("Foundation class")
}

fn query(key: &str) -> Retained<AnyObject> {
    // SAFETY: The dictionary retains all its Foundation/CFString keys and values.
    unsafe {
        let query: Retained<AnyObject> = msg_send![class(c"NSMutableDictionary"), new];
        let _: () = msg_send![&*query, setObject: kSecClassGenericPassword, forKey: kSecClass];
        let _: () =
            msg_send![&*query, setObject: &*NSString::from_str(SERVICE), forKey: kSecAttrService];
        let _: () =
            msg_send![&*query, setObject: &*NSString::from_str(key), forKey: kSecAttrAccount];
        query
    }
}

fn dictionary(value: &AnyObject) -> *const c_void {
    (value as *const AnyObject).cast()
}

fn secret(value: Option<String>) -> AiSecret {
    AiSecret {
        ok: value.is_some(),
        value: value.unwrap_or_default(),
    }
}
