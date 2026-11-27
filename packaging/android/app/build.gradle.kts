plugins {
    id("com.android.application") version "8.7.0"
}

android {
    namespace = "io.parallaxe.box"
    compileSdk = 35

    defaultConfig {
        applicationId = "io.parallaxe.box"
        minSdk = 26
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            // x86_64 + arm64-v8a：MuMu 是 x86_64 宿主，同时原生支持 arm64（abi list 含 arm64-v8a）。
            abiFilters += listOf("x86_64", "arm64-v8a")
        }
    }

    sourceSets {
        getByName("main") {
            manifest.srcFile("../AndroidManifest.xml")
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }
}

dependencies {
    // GameActivity 继承 androidx.appcompat.app.AppCompatActivity。
    // games-activity 的 POM 未声明依赖，AppCompat 必须显式引入，否则运行期
    // NoClassDefFoundError / 主题缺失。
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
}
