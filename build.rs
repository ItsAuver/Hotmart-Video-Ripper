#[cfg(target_os = "windows")]
use winres::WindowsResource;

fn main() {
    #[cfg(target_os = "windows")]

    // Add linker flags for additional hardening
    println!("cargo:rustc-link-arg=-Wl,--gc-sections");  // Remove unused sections
    println!("cargo:rustc-link-arg=-Wl,--strip-all");    // Strip all symbols

    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/CETCOMPAT");      // Enable CET
        println!("cargo:rustc-link-arg=/HIGHENTROPYVA");  // High entropy ASLR
        println!("cargo:rustc-link-arg=/DYNAMICBASE");    // ASLR
        println!("cargo:rustc-link-arg=/NXCOMPAT");       // DEP
    }

    {
        let mut res = WindowsResource::new();

        // Enable ASLR, DEP, and other security features
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
        <security>
            <requestedPrivileges>
                <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
            </requestedPrivileges>
        </security>
    </trustInfo>
    <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
        <application>
            <!-- Windows 10 and Windows 11 -->
            <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
            <!-- Windows 8.1 -->
            <supportedOS Id="{1f676c76-80e1-4239-95bb-83d0f6d0da78}"/>
            <!-- Windows 8 -->
            <supportedOS Id="{4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38}"/>
            <!-- Windows 7 -->
            <supportedOS Id="{35138b9a-5d96-4fbd-8e2d-a2440225f93a}"/>
        </application>
    </compatibility>
    <application xmlns="urn:schemas-microsoft-com:asm.v3">
        <windowsSettings>
            <!-- Enable ASLR, DEP, and other security features -->
            <heapType xmlns="http://schemas.microsoft.com/SMI/2020/WindowsSettings">SegmentHeap</heapType>
            <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
            <!-- Enable DPI awareness -->
            <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true</dpiAware>
            <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
        </windowsSettings>
    </application>
</assembly>"#);

        // Add additional compiler flags for security
        println!("cargo:rustc-link-arg=/CETCOMPAT");  // Enable CET
        println!("cargo:rustc-link-arg=/HIGHENTROPYVA");  // Enable high-entropy ASLR

        res.compile().unwrap();
    }
}
