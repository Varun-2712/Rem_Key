import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.android.library)
}

// The Rust workspace root, one level above the Gradle root.
val repoRoot: File = rootDir.parentFile
val cargo: String = providers.gradleProperty("remkey.cargo")
    .orElse("${System.getProperty("user.home")}/.cargo/bin/cargo")
    .get()
// arm64-v8a covers real phones; x86_64 is the emulator on Intel hosts.
val rustAbis: List<String> = providers.gradleProperty("remkey.rustAbis")
    .orElse("arm64-v8a,x86_64")
    .get()
    .split(',')
    .map(String::trim)

val rustJniLibs = layout.buildDirectory.dir("rustJniLibs")
val uniffiKotlin = layout.buildDirectory.dir("generated/uniffi/kotlin")

kotlin {
    androidTarget {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_17)
        }
    }
    // iOS targets come with the iOS bridge milestone; see Rem_Key_Mobile_Architecture.md §4.3.

    sourceSets {
        commonMain.dependencies {
            implementation(libs.kotlinx.coroutines.core)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation(libs.kotlinx.coroutines.test)
        }
        androidMain {
            kotlin.srcDir(uniffiKotlin)
            dependencies {
                implementation(libs.kotlinx.coroutines.android)
                // The UniFFI Kotlin bindings call into libpir_mobile.so through JNA.
                implementation("net.java.dev.jna:jna:${libs.versions.jna.get()}@aar")
            }
        }
    }
}

android {
    namespace = "dev.remkey.shared"
    compileSdk = libs.versions.compileSdk.get().toInt()
    ndkVersion = libs.versions.ndk.get()

    defaultConfig {
        minSdk = libs.versions.minSdk.get().toInt()
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    sourceSets["main"].jniLibs.srcDir(rustJniLibs)
}

val rustSources = fileTree(repoRoot) {
    include("Cargo.toml", "Cargo.lock", "core/**", "mobile/rust-bridge/**")
    exclude("**/target/**")
}

val cargoBuildAndroid by tasks.registering(Exec::class) {
    group = "rust"
    description = "Builds libpir_mobile.so for ${rustAbis.joinToString()} with cargo-ndk."
    inputs.files(rustSources)
    inputs.property("abis", rustAbis)
    outputs.dir(rustJniLibs)
    workingDir = repoRoot
    doFirst {
        // Resolved here, not at configuration time, so a missing NDK only fails Rust builds.
        environment("ANDROID_NDK_HOME", android.ndkDirectory.absolutePath)
    }
    commandLine(
        listOf(cargo, "ndk") +
            rustAbis.flatMap { listOf("-t", it) } +
            listOf("-o", rustJniLibs.get().asFile.absolutePath, "build", "-p", "pir_mobile", "--release"),
    )
}

val generateUniffiBindings by tasks.registering(Exec::class) {
    group = "rust"
    description = "Generates the Kotlin bindings for pir_mobile from the built library."
    dependsOn(cargoBuildAndroid)
    inputs.files(rustSources)
    outputs.dir(uniffiKotlin)
    workingDir = repoRoot
    doFirst { delete(uniffiKotlin) }
    // Any ABI's library carries the same metadata; take the first one built.
    val library = rustJniLibs.map { it.file("${rustAbis.first()}/libpir_mobile.so").asFile.absolutePath }
    commandLine(
        cargo, "run", "-q", "-p", "uniffi-bindgen", "--",
        "generate", "--library", library.get(),
        "--crate", "pir_mobile", "--language", "kotlin", "--no-format",
        "--out-dir", uniffiKotlin.get().asFile.absolutePath,
    )
}

// srcDir(uniffiKotlin) above is a plain directory, so wire the task in explicitly.
tasks.named("preBuild") { dependsOn(generateUniffiBindings) }
