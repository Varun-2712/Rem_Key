# JNA looks up native callbacks and structures reflectively.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-dontwarn java.awt.**
# Generated UniFFI bindings are loaded by name from the Rust library.
-keep class dev.remkey.bridge.** { *; }
