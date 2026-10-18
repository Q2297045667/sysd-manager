#!/usr/bin/env python3
"""Launch the compiled UI suite without touching the user's desktop or services."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main(binary):
    with tempfile.TemporaryDirectory(prefix="sysd-ui-tests-") as temporary:
        directory = Path(temporary)
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("LC_") and key not in {"LANGUAGE", "LOCPATH", "DISPLAY", "WAYLAND_DISPLAY"}}
        environment.update(
            SYSD_UI_TEST_ISOLATED="1", GDK_BACKEND="broadway", BROADWAY_DISPLAY=":55",
            GSK_RENDERER="cairo", G_DEBUG="fatal-warnings", GTK_A11Y="none",
            GIO_USE_VFS="local", GIO_USE_PROXY_RESOLVER="dummy", GSETTINGS_BACKEND="memory",
            XDG_RUNTIME_DIR=str(directory), XDG_CONFIG_HOME=str(directory / "config"),
            XDG_DATA_HOME=str(directory / "data"), XDG_CACHE_HOME=str(directory / "cache"),
            TEXTDOMAINDIR=os.environ["SYSD_TEST_LOCALE_DIR"], LANG="en_US.UTF-8", LC_ALL="en_US.UTF-8",
        )
        runtime = os.environ.get("SYSD_GTK_RUNTIME")
        backend = "gtk4-broadwayd"
        if runtime:
            runtime = Path(runtime).resolve()
            library = runtime / "usr/lib"
            environment["LD_LIBRARY_PATH"] = str(library) + os.pathsep + environment.get("LD_LIBRARY_PATH", "")
            backend = str(runtime / "usr/bin/gtk4-broadwayd")
        available = subprocess.check_output(["locale", "-a"], text=True, env=environment).lower().replace("-", "")
        if "en_us.utf8" not in available:
            locales = directory / "locales"
            locales.mkdir()
            subprocess.run(["localedef", "--no-archive", "-i", "en_US", "-f", "UTF-8", str(locales / "en_US.UTF-8")], check=True)
            environment["LOCPATH"] = str(locales)
        schemas = directory / "schemas"
        schemas.mkdir()
        shutil.copy(ROOT / "data/schemas/io.github.plrigaux.sysd-manager.gschema.xml", schemas)
        subprocess.run(["glib-compile-schemas", "--strict", str(schemas)], check=True)
        environment["GSETTINGS_SCHEMA_DIR"] = str(schemas)
        config = directory / "bus.conf"
        config.write_text(f'''<busconfig><type>session</type><listen>unix:tmpdir={directory}</listen>
        <auth>EXTERNAL</auth><policy context="default"><allow own="*"/>
        <allow send_destination="*"/><allow receive_sender="*"/></policy></busconfig>''')
        with (directory / "display.log").open("w+") as display_log:
            display = subprocess.Popen([backend, "--port=0", ":55"], env=environment,
                                       stdout=display_log, stderr=subprocess.STDOUT)
            bus = subprocess.Popen(["dbus-daemon", "--nofork", "--print-address=1", "--config-file=" + str(config)],
                                   env=environment, stdout=subprocess.PIPE, text=True)
            try:
                address = bus.stdout.readline().strip()
                if not address:
                    raise RuntimeError("private D-Bus daemon did not start")
                environment["DBUS_SESSION_BUS_ADDRESS"] = address
                environment["DBUS_SYSTEM_BUS_ADDRESS"] = address
                # The backend readiness check is bounded; stderr stays available on failure.
                import time
                until = time.monotonic() + 5
                while not (directory / "broadway56.socket").exists():
                    if display.poll() is not None or time.monotonic() >= until:
                        raise RuntimeError("private GTK display did not start")
                    time.sleep(0.01)
                environment["LANGUAGE"] = "en"
                try:
                    subprocess.run([binary, "--preflight"], env=environment, check=True, timeout=20)
                except subprocess.CalledProcessError as error:
                    raise RuntimeError("GTK window/popover preflight failed before application tests. "
                                       "Use a compatible GTK/libadwaita runtime via SYSD_GTK_RUNTIME; "
                                       "fatal warnings remain enabled.") from error
                for language in ["en", "zh_CN"]:
                    environment["LANGUAGE"] = language
                    subprocess.run([binary], env=environment, check=True, timeout=45)
            finally:
                for process in [bus, display]:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                display_log.seek(0)
                log = display_log.read()
                if "ERROR" in log or "assertion" in log:
                    raise RuntimeError("GTK display preflight/rendering failed:\n" + log)


if __name__ == "__main__":
    main(sys.argv[1])
