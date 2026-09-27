// Extracts the test cases of PapaParse's tests/test-cases.js as JSON.
//
// Usage: node extract-papaparse.js path/to/test-cases.js > test-cases.json
const fs = require('fs');
const vm = require('vm');

const source = fs.readFileSync(process.argv[2], 'utf8');

function extract(name) {
  const start = source.indexOf('var ' + name + ' = [');
  if (start < 0) throw new Error('missing ' + name);
  const end = source.indexOf('\n];', start);
  const code = source.slice(start + ('var ' + name + ' = ').length, end + 2);
  const context = {
    RECORD_SEP: String.fromCharCode(30),
    UNIT_SEP: String.fromCharCode(31),
    BASE_PATH: '',
    FILES_ENABLED: false,
    XHR_ENABLED: false,
  };
  return vm.runInNewContext('(' + code + ')', context);
}

// only plain JSON values can be compared, cases with functions or dates
// are left out
function isPlain(value) {
  if (value === null || ['string', 'number', 'boolean', 'undefined'].includes(typeof value)) {
    return true;
  }
  if (Array.isArray(value)) return value.every(isPlain);
  // the values come from another context, their prototypes differ
  if (Object.prototype.toString.call(value) === '[object Object]') {
    return Object.values(value).every(isPlain);
  }
  return false;
}

const out = [];
for (const [suite, name] of [['core', 'CORE_PARSER_TESTS'], ['parse', 'PARSE_TESTS'], ['unparse', 'UNPARSE_TESTS']]) {
  const seen = {};
  for (const test of extract(name)) {
    if (test.disabled || !isPlain(test.input) || !isPlain(test.config) || !isPlain(test.expected)) {
      continue;
    }
    // cases with generated large inputs are left out
    if (JSON.stringify(test).length > 16384) {
      continue;
    }
    let id = suite + ': ' + test.description;
    seen[id] = (seen[id] || 0) + 1;
    if (seen[id] > 1) id += ' #' + seen[id];
    out.push({
      name: id,
      input: test.input,
      config: test.config || {},
      expected: test.expected,
    });
  }
}
// one case per line
process.stdout.write('[\n' + out.map((test) => JSON.stringify(test)).join(',\n') + '\n]\n');
