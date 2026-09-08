from pathlib import Path
import json, shlex, subprocess, sys
root = Path(sys.argv[1] if len(sys.argv) > 1 else "/Users/nyabi/Documents/Code/Ugurugu").resolve()
build = root / "out/build/macos-release"
output = Path(__file__).resolve().parent
entry = next(x for x in json.loads((build / "compile_commands.json").read_text()) if x["file"].endswith("tests/TestMain.cpp"))
args = shlex.split(entry["command"])
args[args.index("-o") + 1] = str(output / "probe.o")
args[-1] = str(output / "probe.cpp")
subprocess.run(args, cwd=build, check=True)
line = subprocess.check_output(["ninja", "-C", str(build), "-t", "commands", "ugurugu_tests"], text=True).splitlines()[-1]
args = [x for x in shlex.split(line)[2:-2] if not x.startswith("CMakeFiles/ugurugu_tests.dir/")]
args[args.index("-o") + 1] = str(output / "probe")
args.insert(args.index("-o"), str(output / "probe.o"))
subprocess.run(args, cwd=build, check=True)
result = subprocess.run([str(output / "probe")], cwd=build, text=True, capture_output=True, timeout=45)
(output / "probe.log").write_text(result.stdout + result.stderr)
print(result.stdout)
print(result.stderr, file=sys.stderr)
sys.exit(result.returncode)
