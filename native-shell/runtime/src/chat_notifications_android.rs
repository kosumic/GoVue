use govue_core::ChatNotificationPlatform;
use jni::objects::{GlobalRef, JClass, JObject, JValue};
use jni::{JNIEnv, JavaVM};

struct AndroidNotifications {
    vm: JavaVM,
    adapter: GlobalRef,
}

impl ChatNotificationPlatform for AndroidNotifications {
    fn update(&self, chat_id: String, show: bool, revision: u64) {
        let Ok(mut env) = self.vm.attach_current_thread() else {
            return;
        };
        let result: jni::errors::Result<()> = env.with_local_frame(4, |env| {
            let id = env.new_string(chat_id)?;
            env.call_method(
                self.adapter.as_obj(),
                "update",
                "(Ljava/lang/String;ZJ)V",
                &[
                    JValue::Object(&id),
                    JValue::Bool(u8::from(show)),
                    JValue::Long(revision as i64),
                ],
            )?;
            Ok(())
        });
        if result.is_err() && env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
    }
}

#[unsafe(no_mangle)]
extern "system" fn Java_io_github_kaminarios_govue_shell_ReplyNotifications_installNative(
    env: JNIEnv,
    _class: JClass,
    adapter: JObject,
) {
    if let (Ok(vm), Ok(adapter)) = (env.get_java_vm(), env.new_global_ref(adapter)) {
        govue_core::install_chat_notification_platform(Box::new(AndroidNotifications {
            vm,
            adapter,
        }));
    }
}
