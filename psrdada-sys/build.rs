extern crate bindgen;
extern crate cc;

use std::{env, fs, io::Write, path::PathBuf};

fn main() {
    // Build vendor library
    let mut c = cc::Build::new();
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let config_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("include");
    let vendor = root.join("vendor");

    fs::create_dir_all(&config_dir).unwrap();

    println!(
        "cargo:include={}",
        env::join_paths([&config_dir, &vendor])
            .unwrap()
            .to_str()
            .unwrap()
    );
    c.include(&config_dir);
    c.include(&vendor);
    c.pic(true);

    if let Ok(target_cpu) = env::var("TARGET_CPU") {
        c.flag_if_supported(&format!("-march={}", target_cpu));
    }

    c.warnings(false);

    let mut config_h = fs::File::create(config_dir.join("config.h")).unwrap();
    // Are these all decent assumptions to make?
    write!(
        config_h,
        r#"
        #define HAVE_ALARM 1
        #define HAVE_ARPA_INET_H 1
        #define HAVE_DLFCN_H 1
        #define HAVE_FCNTL_H 1
        #define HAVE_FORK 1
        #define HAVE_GETHOSTBYNAME 1
        #define HAVE_GETHOSTNAME 1
        #define HAVE_GETTIMEOFDAY 1
        #define HAVE_INET_NTOA 1
        #define HAVE_INTTYPES_H 1
        #define HAVE_LIBM 1
        #define HAVE_MALLOC 1
        #define HAVE_MEMSET 1
        #define HAVE_NETDB_H 1
        #define HAVE_NETINET_IN_H 1
        #define HAVE_PTHREAD 1
        #define HAVE_REALLOC 1
        #define HAVE_SELECT 1
        #define HAVE_SOCKET 1
        #define HAVE_STDINT_H 1
        #define HAVE_STDIO_H 1
        #define HAVE_STDLIB_H 1
        #define HAVE_STRCHR 1
        #define HAVE_STRCSPN 1
        #define HAVE_STRDUP 1
        #define HAVE_STRERROR 1
        #define HAVE_STRFTIME 1
        #define HAVE_STRINGS_H 1
        #define HAVE_STRING_H 1
        #define HAVE_STRSTR 1
        #define HAVE_SYSLOG_H 1
        #define HAVE_SYS_IOCTL_H 1
        #define HAVE_SYS_MOUNT_H 1
        #define HAVE_SYS_SELECT_H 1
        #define HAVE_SYS_SOCKET_H 1
        #define HAVE_SYS_STATVFS_H 1
        #define HAVE_SYS_STAT_H 1
        #define HAVE_SYS_TIME_H 1
        #define HAVE_SYS_TYPES_H 1
        #define HAVE_SYS_VFS_H 1
        #define HAVE_UNISTD_H 1
        #define HAVE_VFORK 1
        #define HAVE_VPRINTF 1
        #define HAVE_WORKING_FORK 1
        #define HAVE_WORKING_VFORK 1
        #define LSTAT_FOLLOWS_SLASHED_SYMLINK 1
        #define LT_OBJDIR ".libs/"
        #define PACKAGE "dada"
        #define PACKAGE_BUGREPORT "straten@astron.nl"
        #define PACKAGE_NAME "DADA"
        #define PACKAGE_STRING "DADA 1.0"
        #define PACKAGE_TARNAME "dada"
        #define PACKAGE_URL ""
        #define PACKAGE_VERSION "1.0"
        #define RETSIGTYPE void
        #define SELECT_TYPE_ARG1 int
        #define SELECT_TYPE_ARG234 (fd_set *)
        #define SELECT_TYPE_ARG5 (struct timeval *)
        #define STDC_HEADERS 1
        #define TIME_WITH_SYS_TIME 1
        #define VERSION "1.0"
        "#,
    )
    .unwrap();

    // All the source files
    c.include("vendor/src")
        .file("vendor/src/ascii_header.c")
        .file("vendor/src/command_parse.c")
        .file("vendor/src/command_parse_server.c")
        .file("vendor/src/dada_affinity.c")
        .file("vendor/src/dada_client.c")
        .file("vendor/src/dada_generator.c")
        .file("vendor/src/dada_hdu.c")
        .file("vendor/src/dada_ni.c")
        .file("vendor/src/dada_pwc.c")
        .file("vendor/src/dada_pwc_main.c")
        .file("vendor/src/dada_pwc_main_multi.c")
        .file("vendor/src/dada_pwc_nexus.c")
        .file("vendor/src/dada_pwc_nexus_config.c")
        .file("vendor/src/dada_pwc_nexus_header_parse.c")
        .file("vendor/src/dada_udp.c")
        .file("vendor/src/daemon.c")
        .file("vendor/src/diff_time.c")
        .file("vendor/src/disk_array.c")
        .file("vendor/src/fileread.c")
        .file("vendor/src/filesize.c")
        .file("vendor/src/ipcbuf.c")
        .file("vendor/src/ipcio.c")
        .file("vendor/src/ipcutil.c")
        .file("vendor/src/mach_gettime.c")
        .file("vendor/src/monitor.c")
        .file("vendor/src/multilog.c")
        .file("vendor/src/multilog_server.c")
        .file("vendor/src/nexus.c")
        .file("vendor/src/node_array.c")
        .file("vendor/src/sock.c")
        .file("vendor/src/string_array.c")
        .file("vendor/src/stopwatch.c")
        .file("vendor/src/tmutil.c");

    // Compile
    c.compile("psrdada");

    // ------ BINDGEN
    // Tell cargo to invalidate the built crate whenever the wrapper changes
    println!("cargo:rerun-if-changed=wrapper.h");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks))
        // Tell bindgen about the structs which have mutexes, so they don't `copy`
        .no_copy("multilog_t")
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");
}
