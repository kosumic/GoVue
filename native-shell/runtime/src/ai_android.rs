use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicI64, Ordering},
};

use govue_core::{AiHttpRequest, AiPlatform, AiResponse, AiSecret};
use jni::errors::Result;
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue};
use jni::sys::{JNI_FALSE, JNI_TRUE, jboolean, jint, jlong};
use jni::{JNIEnv, JavaVM};
use serde_json::json;

static NEXT_REQUEST: AtomicI64 = AtomicI64::new(1);
static RESPONSES: OnceLock<Mutex<HashMap<jlong, Arc<AiResponse>>>> = OnceLock::new();

struct AndroidAi {
    vm: JavaVM,
    adapter: GlobalRef,
}

impl AndroidAi {
    fn java<T>(&self, work: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>) -> Option<T> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let result = env.with_local_frame(16, |env| work(env, self.adapter.as_obj()));
        if result.is_err() && env.exception_check().ok()? {
            env.exception_clear().ok()?;
        }
        result.ok()
    }
}

impl AiPlatform for AndroidAi {
    fn secret_read(&self, key: String) -> AiSecret {
        let value = self
            .java(|env, adapter| {
                let key = env.new_string(key)?;
                let value = env
                    .call_method(
                        adapter,
                        "readSecret",
                        "(Ljava/lang/String;)Ljava/lang/String;",
                        &[JValue::Object(&key)],
                    )?
                    .l()?;
                let value = JString::from(value);
                Ok((!value.is_null())
                    .then(|| env.get_string(&value).map(String::from))
                    .transpose()?)
            })
            .flatten();
        AiSecret {
            ok: value.is_some(),
            value: value.unwrap_or_default(),
        }
    }

    fn secret_write(&self, key: String, value: String) -> bool {
        self.java(|env, adapter| {
            let key = env.new_string(key)?;
            let value = env.new_string(value)?;
            env.call_method(
                adapter,
                "writeSecret",
                "(Ljava/lang/String;Ljava/lang/String;)Z",
                &[JValue::Object(&key), JValue::Object(&value)],
            )?
            .z()
        })
        .unwrap_or(false)
    }

    fn request(&self, request: AiHttpRequest, response: Arc<AiResponse>) {
        let Some(handle) = RequestId::new(response.clone()) else {
            response.failed();
            return;
        };
        let headers = request
            .headers
            .into_iter()
            .map(|header| json!({"name":header.name,"value":header.value}))
            .collect::<Vec<_>>();
        let done = self
            .java(|env, adapter| {
                let url = env.new_string(request.url)?;
                let method = env.new_string(request.method)?;
                let headers = env.new_string(json!(headers).to_string())?;
                let body = env.new_string(request.body)?;
                env.call_method(
                    adapter,
                    "request",
                    "(JLjava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                    &[
                        JValue::Long(handle.0),
                        JValue::Object(&url),
                        JValue::Object(&method),
                        JValue::Object(&headers),
                        JValue::Object(&body),
                    ],
                )?
                .z()
            })
            .unwrap_or(false);
        if !done && !response.is_cancelled() {
            response.failed();
        }
    }
}

struct RequestId(jlong);

impl RequestId {
    fn new(response: Arc<AiResponse>) -> Option<Self> {
        let request = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
        RESPONSES
            .get_or_init(Default::default)
            .lock()
            .ok()?
            .insert(request, response);
        Some(Self(request))
    }
}

impl Drop for RequestId {
    fn drop(&mut self) {
        if let Some(responses) = RESPONSES.get()
            && let Ok(mut responses) = responses.lock()
        {
            responses.remove(&self.0);
        }
    }
}

fn response(id: jlong) -> Option<Arc<AiResponse>> {
    RESPONSES.get()?.lock().ok()?.get(&id).cloned()
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_AiTransport_installNative(
    env: JNIEnv,
    _class: JClass,
    adapter: JObject,
) {
    if let (Ok(vm), Ok(adapter)) = (env.get_java_vm(), env.new_global_ref(adapter)) {
        govue_core::install_ai_platform(Box::new(AndroidAi { vm, adapter }));
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_AiTransport_active(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
) -> jboolean {
    if response(id).is_some_and(|response| !response.is_cancelled()) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_AiTransport_start(
    _env: JNIEnv,
    _class: JClass,
    id: jlong,
    status: jint,
) {
    if let Some(response) = response(id) {
        response.start(status);
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_AiTransport_push(
    env: JNIEnv,
    _class: JClass,
    id: jlong,
    bytes: JByteArray,
    count: jint,
) -> jboolean {
    if let Some(response) = response(id)
        && (1..=16 * 1024).contains(&count)
        && let Ok(mut bytes) = env.convert_byte_array(bytes)
        && bytes.len() >= count as usize
    {
        bytes.truncate(count as usize);
        if response.push(bytes) {
            return JNI_TRUE;
        }
    }
    JNI_FALSE
}
