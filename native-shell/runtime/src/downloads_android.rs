use std::{ptr, sync::Arc};

use govue_core::{DownloadHttp, DownloadPlatform, DownloadRequest, MediaRangeReply};
use jni::{
    JNIEnv, JavaVM,
    errors::Result,
    objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue},
    sys::jstring,
};

#[derive(Clone)]
struct AndroidDownloads {
    vm: Arc<JavaVM>,
    adapter: GlobalRef,
}

impl AndroidDownloads {
    fn java<T>(&self, work: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>) -> Option<T> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let result = env.with_local_frame(16, |env| work(env, self.adapter.as_obj()));
        if result.is_err() && env.exception_check().ok()? {
            env.exception_clear().ok()?;
        }
        result.ok()
    }

    fn path(&self, method: &str) -> String {
        self.java(|env, adapter| {
            let value = env
                .call_method(adapter, method, "()Ljava/lang/String;", &[])?
                .l()?;
            Ok(env.get_string(&JString::from(value))?.into())
        })
        .unwrap_or_default()
    }
}

impl DownloadPlatform for AndroidDownloads {
    fn directory(&self) -> String {
        self.path("directory")
    }

    fn export_directory(&self) -> String {
        self.path("exportDirectory")
    }

    fn supported_formats(&self, formats: String) -> String {
        self.java(|env, adapter| {
            let formats = env.new_string(formats)?;
            let value = env
                .call_method(
                    adapter,
                    "formats",
                    "(Ljava/lang/String;)Ljava/lang/String;",
                    &[JValue::Object(&formats)],
                )?
                .l()?;
            Ok(env.get_string(&JString::from(value))?.into())
        })
        .unwrap_or_else(|| "[]".to_owned())
    }

    fn mux(&self, video: String, audio: String, output: String) -> bool {
        self.java(|env, adapter| {
            let video = env.new_string(video)?;
            let audio = env.new_string(audio)?;
            let output = env.new_string(output)?;
            env.call_method(
                adapter,
                "mux",
                "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)Z",
                &[
                    JValue::Object(&video),
                    JValue::Object(&audio),
                    JValue::Object(&output),
                ],
            )?
            .z()
        })
        .unwrap_or(false)
    }

    fn publish(&self, id: String) -> i32 {
        self.java(|env, adapter| {
            let id = env.new_string(id)?;
            env.call_method(
                adapter,
                "publish",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&id)],
            )?
            .i()
        })
        .unwrap_or(-1)
    }

    fn delete_published(&self, uri: String) -> bool {
        self.java(|env, adapter| {
            let uri = env.new_string(uri)?;
            env.call_method(
                adapter,
                "deletePublished",
                "(Ljava/lang/String;)Z",
                &[JValue::Object(&uri)],
            )?
            .z()
        })
        .unwrap_or(false)
    }

    fn activity(&self, count: u32, title: String, received: u64, total: u64) {
        let _ = self.java(|env, adapter| {
            let title = env.new_string(title)?;
            env.call_method(
                adapter,
                "activity",
                "(ILjava/lang/String;JJ)V",
                &[
                    JValue::Int(count.min(2000) as i32),
                    JValue::Object(&title),
                    JValue::Long(received.min(200_000_000_000) as i64),
                    JValue::Long(total.min(200_000_000_000) as i64),
                ],
            )?
            .v()
        });
    }
}

impl DownloadHttp for AndroidDownloads {
    fn wifi_connected(&self) -> bool {
        self.java(|env, adapter| env.call_method(adapter, "wifiConnected", "()Z", &[])?.z())
            .unwrap_or(false)
    }

    fn fetch(&self, request: DownloadRequest) -> MediaRangeReply {
        let failure = || MediaRangeReply {
            status: 0,
            body: Vec::new(),
            location: None,
        };
        let (first, last) = match request.range {
            None => (-1, -1),
            Some(range) => match (i64::try_from(range.start), i64::try_from(range.end)) {
                (Ok(first), Ok(last)) => (first, last),
                _ => return failure(),
            },
        };
        self.java(|env, adapter| {
            let url = env.new_string(request.url)?;
            let agent = env.new_string(request.user_agent)?;
            let origin = env.new_string(request.origin)?;
            let method = env.new_string(request.method)?;
            let reply = env
                .call_method(
                    adapter,
                    "fetch",
                    concat!(
                        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;JJZ)",
                        "Lio/github/kaminarios/govue/coremodule/AndroidRangeReply;"
                    ),
                    &[
                        JValue::Object(&url),
                        JValue::Object(&agent),
                        JValue::Object(&origin),
                        JValue::Object(&method),
                        JValue::Long(first),
                        JValue::Long(last),
                        JValue::Bool(u8::from(request.wifi_only)),
                    ],
                )?
                .l()?;
            let status = env.call_method(&reply, "getStatus", "()I", &[])?.i()?;
            let body = env.call_method(&reply, "getBody", "()[B", &[])?.l()?;
            let location = env
                .call_method(&reply, "getLocation", "()Ljava/lang/String;", &[])?
                .l()?;
            let location = JString::from(location);
            Ok(MediaRangeReply {
                status,
                body: env.convert_byte_array(JByteArray::from(body))?,
                location: if location.is_null() {
                    None
                } else {
                    Some(env.get_string(&location)?.into())
                },
            })
        })
        .unwrap_or_else(failure)
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_DownloadTransport_installNative(
    env: JNIEnv,
    _class: JClass,
    adapter: JObject,
) {
    if let (Ok(vm), Ok(adapter)) = (env.get_java_vm(), env.new_global_ref(adapter)) {
        let backend = AndroidDownloads {
            vm: Arc::new(vm),
            adapter,
        };
        govue_core::install_download_platform(Box::new(backend.clone()));
        govue_core::install_download_http(Box::new(backend));
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_DownloadTransport_execute(
    mut env: JNIEnv,
    _class: JClass,
    command: JString,
    root: JString,
) -> jstring {
    let request = (|| {
        let command = env.get_string(&command)?.into();
        let root = env.get_string(&root)?.into();
        env.new_string(govue_core::execute(command, root))
    })();
    request.map_or(ptr::null_mut(), JString::into_raw)
}
