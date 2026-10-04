package gcfg

// Added to gcfg by scripts/update-ini-test-data.sh: read() calls
// captureInput with every input, which is appended to the JSON lines file
// named by INI_CAPTURE together with the name of the test that parsed it.

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"runtime"
	"strings"
	"sync"
)

var captureMu sync.Mutex

func captureInput(src []byte) {
	path := os.Getenv("INI_CAPTURE")
	if path == "" {
		return
	}
	record, err := json.Marshal(map[string]string{
		"test":  currentTest(),
		"input": base64.StdEncoding.EncodeToString(src),
	})
	if err != nil {
		panic(err)
	}
	captureMu.Lock()
	defer captureMu.Unlock()
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o644)
	if err != nil {
		panic(err)
	}
	defer f.Close()
	if _, err := f.Write(append(record, '\n')); err != nil {
		panic(err)
	}
}

// currentTest returns the innermost test or example function on the stack
// without its package, for instance "TestReadStringInto" or
// "(*DecoderSuite).TestDecode".
func currentTest() string {
	pc := make([]uintptr, 64)
	frames := runtime.CallersFrames(pc[:runtime.Callers(3, pc)])
	for {
		frame, more := frames.Next()
		name := frame.Function[strings.LastIndex(frame.Function, "/")+1:]
		parts := strings.Split(name, ".")
		for i, part := range parts {
			if i > 0 && (strings.HasPrefix(part, "Test") || strings.HasPrefix(part, "Example")) {
				return strings.Join(parts[1:i+1], ".")
			}
		}
		if !more {
			return ""
		}
	}
}
