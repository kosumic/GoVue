use std::sync::Arc;

use govue_core::CastPlatform;
use jni::{
    JNIEnv, JavaVM,
    errors::Result,
    objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue},
    sys::jboolean,
};

struct AndroidCast {
    vm: Arc<JavaVM>,
    adapter: GlobalRef,
}

impl AndroidCast {
    fn java<T>(&self, work: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T>) -> Option<T> {
        let mut env = self.vm.attach_current_thread().ok()?;
        let result = env.with_local_frame(16, |env| work(env, self.adapter.as_obj()));
        if result.is_err() && env.exception_check().ok()? {
            env.exception_clear().ok()?;
        }
        result.ok()
    }
}

impl CastPlatform for AndroidCast {
    fn wifi_address(&self) -> String {
        self.java(|env, adapter| {
            let result = env
                .call_method(adapter, "wifiAddress", "()Ljava/lang/String;", &[])?
                .l()?;
            Ok(env.get_string(&JString::from(result))?.into())
        })
        .unwrap_or_default()
    }

    fn bind_wifi_socket(&self, socket_fd: u32, address: String) -> bool {
        let Ok(fd) = i32::try_from(socket_fd) else {
            return false;
        };
        self.java(|env, adapter| {
            let address = env.new_string(address)?;
            env.call_method(
                adapter,
                "bindWifiSocket",
                "(ILjava/lang/String;)Z",
                &[JValue::Int(fd), JValue::Object(&address)],
            )?
            .z()
        })
        .unwrap_or(false)
    }

    fn connect(&self, host: String, port: u16) -> String {
        self.java(|env, adapter| {
            let host = env.new_string(host)?;
            let result = env
                .call_method(
                    adapter,
                    "connect",
                    "(Ljava/lang/String;I)Ljava/lang/String;",
                    &[JValue::Object(&host), JValue::Int(i32::from(port))],
                )?
                .l()?;
            Ok(env.get_string(&JString::from(result))?.into())
        })
        .unwrap_or_default()
    }

    fn read(&self, socket: String) -> Vec<u8> {
        self.java(|env, adapter| {
            let socket = env.new_string(socket)?;
            let result = env
                .call_method(
                    adapter,
                    "read",
                    "(Ljava/lang/String;)[B",
                    &[JValue::Object(&socket)],
                )?
                .l()?;
            env.convert_byte_array(JByteArray::from(result))
        })
        .unwrap_or_default()
    }

    fn write(&self, socket: String, frame: Vec<u8>) -> bool {
        self.java(|env, adapter| {
            let socket = env.new_string(socket)?;
            let frame = env.byte_array_from_slice(&frame)?;
            env.call_method(
                adapter,
                "write",
                "(Ljava/lang/String;[B)Z",
                &[JValue::Object(&socket), JValue::Object(&frame)],
            )?
            .z()
        })
        .unwrap_or(false)
    }

    fn close(&self, socket: String) {
        let _ = self.java(|env, adapter| {
            let socket = env.new_string(socket)?;
            env.call_method(
                adapter,
                "close",
                "(Ljava/lang/String;)V",
                &[JValue::Object(&socket)],
            )?
            .v()
        });
    }

    fn activity(&self, active: bool) -> bool {
        self.java(|env, adapter| {
            env.call_method(
                adapter,
                "activity",
                "(Z)Z",
                &[JValue::Bool(jboolean::from(active))],
            )?
            .z()
        })
        .unwrap_or(false)
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kaminarios_govue_shell_CastTransport_installNative(
    env: JNIEnv,
    _class: JClass,
    platform: JObject,
) {
    if let (Ok(vm), Ok(adapter)) = (env.get_java_vm(), env.new_global_ref(platform)) {
        govue_core::install_cast_platform(Box::new(AndroidCast {
            vm: Arc::new(vm),
            adapter,
        }));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_kaminarios_govue_shell_CastTransport_stopNative(
    mut env: JNIEnv,
    _class: JClass,
    root: JString,
) {
    if let Ok(root) = env.get_string(&root) {
        let _ = govue_core::execute(r#"{"op":"cast_stop_all"}"#.into(), root.into());
    }
}
