# UniFFI uses JNA reflection to resolve native functions, callbacks and structure
# fields. Preserve the generated binding package when the app is shrunk by R8.
-keep class io.github.kaminarios.govue.core.** { *; }

# libjnidispatch also resolves JNA classes and fields (including Pointer.peer)
# by name through JNI, independently of the generated UniFFI bindings.
# https://github.com/java-native-access/jna/blob/master/www/FrequentlyAskedQuestions.md#jna-on-android
-keep class com.sun.jna.** { *; }

# JNA's desktop window helpers are unused on Android.
-dontwarn java.awt.**
