//! The names of Python 2 that Python 3 reads with other names.
//!
//! Python reads the globals of pickles of protocols 0 to 2 with the names
//! of Python 3 (`fix_imports`) and writes the globals of Python 3 with the
//! names of Python 2.  This is a copy of the tables of CPython's
//! `Lib/_compat_pickle.py` (3.14), sorted for binary search.

/// Renamed globals: `(module, name)` to `(module, name)`.
type NameMapping = [((&'static str, &'static str), (&'static str, &'static str))];

/// The names Python 3 reads for the globals of Python 2.
pub(crate) static NAME_MAPPING: &NameMapping = &[
    (
        ("UserDict", "IterableUserDict"),
        ("collections", "UserDict"),
    ),
    (("UserDict", "UserDict"), ("collections", "UserDict")),
    (("UserList", "UserList"), ("collections", "UserList")),
    (("UserString", "UserString"), ("collections", "UserString")),
    (("__builtin__", "basestring"), ("builtins", "str")),
    (("__builtin__", "intern"), ("sys", "intern")),
    (("__builtin__", "long"), ("builtins", "int")),
    (("__builtin__", "reduce"), ("functools", "reduce")),
    (("__builtin__", "unichr"), ("builtins", "chr")),
    (("__builtin__", "unicode"), ("builtins", "str")),
    (("__builtin__", "xrange"), ("builtins", "range")),
    (
        ("_multiprocessing", "Connection"),
        ("multiprocessing.connection", "Connection"),
    ),
    (("_socket", "fromfd"), ("socket", "fromfd")),
    (
        ("exceptions", "ArithmeticError"),
        ("builtins", "ArithmeticError"),
    ),
    (
        ("exceptions", "AssertionError"),
        ("builtins", "AssertionError"),
    ),
    (
        ("exceptions", "AttributeError"),
        ("builtins", "AttributeError"),
    ),
    (
        ("exceptions", "BaseException"),
        ("builtins", "BaseException"),
    ),
    (("exceptions", "BufferError"), ("builtins", "BufferError")),
    (("exceptions", "BytesWarning"), ("builtins", "BytesWarning")),
    (
        ("exceptions", "DeprecationWarning"),
        ("builtins", "DeprecationWarning"),
    ),
    (("exceptions", "EOFError"), ("builtins", "EOFError")),
    (
        ("exceptions", "EnvironmentError"),
        ("builtins", "EnvironmentError"),
    ),
    (("exceptions", "Exception"), ("builtins", "Exception")),
    (
        ("exceptions", "FloatingPointError"),
        ("builtins", "FloatingPointError"),
    ),
    (
        ("exceptions", "FutureWarning"),
        ("builtins", "FutureWarning"),
    ),
    (
        ("exceptions", "GeneratorExit"),
        ("builtins", "GeneratorExit"),
    ),
    (("exceptions", "IOError"), ("builtins", "IOError")),
    (("exceptions", "ImportError"), ("builtins", "ImportError")),
    (
        ("exceptions", "ImportWarning"),
        ("builtins", "ImportWarning"),
    ),
    (
        ("exceptions", "IndentationError"),
        ("builtins", "IndentationError"),
    ),
    (("exceptions", "IndexError"), ("builtins", "IndexError")),
    (("exceptions", "KeyError"), ("builtins", "KeyError")),
    (
        ("exceptions", "KeyboardInterrupt"),
        ("builtins", "KeyboardInterrupt"),
    ),
    (("exceptions", "LookupError"), ("builtins", "LookupError")),
    (("exceptions", "MemoryError"), ("builtins", "MemoryError")),
    (("exceptions", "NameError"), ("builtins", "NameError")),
    (
        ("exceptions", "NotImplementedError"),
        ("builtins", "NotImplementedError"),
    ),
    (("exceptions", "OSError"), ("builtins", "OSError")),
    (
        ("exceptions", "OverflowError"),
        ("builtins", "OverflowError"),
    ),
    (
        ("exceptions", "PendingDeprecationWarning"),
        ("builtins", "PendingDeprecationWarning"),
    ),
    (
        ("exceptions", "ReferenceError"),
        ("builtins", "ReferenceError"),
    ),
    (("exceptions", "RuntimeError"), ("builtins", "RuntimeError")),
    (
        ("exceptions", "RuntimeWarning"),
        ("builtins", "RuntimeWarning"),
    ),
    (("exceptions", "StandardError"), ("builtins", "Exception")),
    (
        ("exceptions", "StopIteration"),
        ("builtins", "StopIteration"),
    ),
    (("exceptions", "SyntaxError"), ("builtins", "SyntaxError")),
    (
        ("exceptions", "SyntaxWarning"),
        ("builtins", "SyntaxWarning"),
    ),
    (("exceptions", "SystemError"), ("builtins", "SystemError")),
    (("exceptions", "SystemExit"), ("builtins", "SystemExit")),
    (("exceptions", "TabError"), ("builtins", "TabError")),
    (("exceptions", "TypeError"), ("builtins", "TypeError")),
    (
        ("exceptions", "UnboundLocalError"),
        ("builtins", "UnboundLocalError"),
    ),
    (
        ("exceptions", "UnicodeDecodeError"),
        ("builtins", "UnicodeDecodeError"),
    ),
    (
        ("exceptions", "UnicodeEncodeError"),
        ("builtins", "UnicodeEncodeError"),
    ),
    (("exceptions", "UnicodeError"), ("builtins", "UnicodeError")),
    (
        ("exceptions", "UnicodeTranslateError"),
        ("builtins", "UnicodeTranslateError"),
    ),
    (
        ("exceptions", "UnicodeWarning"),
        ("builtins", "UnicodeWarning"),
    ),
    (("exceptions", "UserWarning"), ("builtins", "UserWarning")),
    (("exceptions", "ValueError"), ("builtins", "ValueError")),
    (("exceptions", "Warning"), ("builtins", "Warning")),
    (
        ("exceptions", "ZeroDivisionError"),
        ("builtins", "ZeroDivisionError"),
    ),
    (("itertools", "ifilter"), ("builtins", "filter")),
    (("itertools", "ifilterfalse"), ("itertools", "filterfalse")),
    (("itertools", "imap"), ("builtins", "map")),
    (("itertools", "izip"), ("builtins", "zip")),
    (("itertools", "izip_longest"), ("itertools", "zip_longest")),
    (
        ("multiprocessing", "AuthenticationError"),
        ("multiprocessing.context", "AuthenticationError"),
    ),
    (
        ("multiprocessing", "BufferTooShort"),
        ("multiprocessing.context", "BufferTooShort"),
    ),
    (
        ("multiprocessing", "ProcessError"),
        ("multiprocessing.context", "ProcessError"),
    ),
    (
        ("multiprocessing", "TimeoutError"),
        ("multiprocessing.context", "TimeoutError"),
    ),
    (
        ("multiprocessing.forking", "Popen"),
        ("multiprocessing.popen_fork", "Popen"),
    ),
    (
        ("multiprocessing.process", "Process"),
        ("multiprocessing.context", "Process"),
    ),
    (("socket", "_socketobject"), ("socket", "SocketType")),
    (
        ("urllib", "ContentTooShortError"),
        ("urllib.error", "ContentTooShortError"),
    ),
    (("urllib", "getproxies"), ("urllib.request", "getproxies")),
    (
        ("urllib", "pathname2url"),
        ("urllib.request", "pathname2url"),
    ),
    (("urllib", "quote"), ("urllib.parse", "quote")),
    (("urllib", "quote_plus"), ("urllib.parse", "quote_plus")),
    (("urllib", "unquote"), ("urllib.parse", "unquote")),
    (("urllib", "unquote_plus"), ("urllib.parse", "unquote_plus")),
    (
        ("urllib", "url2pathname"),
        ("urllib.request", "url2pathname"),
    ),
    (("urllib", "urlcleanup"), ("urllib.request", "urlcleanup")),
    (("urllib", "urlencode"), ("urllib.parse", "urlencode")),
    (("urllib", "urlopen"), ("urllib.request", "urlopen")),
    (("urllib", "urlretrieve"), ("urllib.request", "urlretrieve")),
    (("urllib2", "HTTPError"), ("urllib.error", "HTTPError")),
    (("urllib2", "URLError"), ("urllib.error", "URLError")),
    (("whichdb", "whichdb"), ("dbm", "whichdb")),
];

/// Renamed modules.
pub(crate) static IMPORT_MAPPING: &[(&str, &str)] = &[
    ("BaseHTTPServer", "http.server"),
    ("CGIHTTPServer", "http.server"),
    ("ConfigParser", "configparser"),
    ("Cookie", "http.cookies"),
    ("Dialog", "tkinter.dialog"),
    ("DocXMLRPCServer", "xmlrpc.server"),
    ("FileDialog", "tkinter.filedialog"),
    ("HTMLParser", "html.parser"),
    ("Queue", "queue"),
    ("ScrolledText", "tkinter.scrolledtext"),
    ("SimpleDialog", "tkinter.simpledialog"),
    ("SimpleHTTPServer", "http.server"),
    ("SimpleXMLRPCServer", "xmlrpc.server"),
    ("SocketServer", "socketserver"),
    ("StringIO", "io"),
    ("Tkconstants", "tkinter.constants"),
    ("Tkdnd", "tkinter.dnd"),
    ("Tkinter", "tkinter"),
    ("UserDict", "collections"),
    ("UserList", "collections"),
    ("UserString", "collections"),
    ("__builtin__", "builtins"),
    ("_abcoll", "collections.abc"),
    ("_elementtree", "xml.etree.ElementTree"),
    ("_winreg", "winreg"),
    ("anydbm", "dbm"),
    ("cPickle", "pickle"),
    ("cStringIO", "io"),
    ("commands", "subprocess"),
    ("cookielib", "http.cookiejar"),
    ("copy_reg", "copyreg"),
    ("dbhash", "dbm.bsd"),
    ("dbm", "dbm.ndbm"),
    ("dumbdbm", "dbm.dumb"),
    ("dummy_thread", "_dummy_thread"),
    ("gdbm", "dbm.gnu"),
    ("htmlentitydefs", "html.entities"),
    ("httplib", "http.client"),
    ("markupbase", "_markupbase"),
    ("repr", "reprlib"),
    ("robotparser", "urllib.robotparser"),
    ("test.test_support", "test.support"),
    ("thread", "_thread"),
    ("tkColorChooser", "tkinter.colorchooser"),
    ("tkCommonDialog", "tkinter.commondialog"),
    ("tkFileDialog", "tkinter.filedialog"),
    ("tkFont", "tkinter.font"),
    ("tkMessageBox", "tkinter.messagebox"),
    ("tkSimpleDialog", "tkinter.simpledialog"),
    ("ttk", "tkinter.ttk"),
    ("urllib2", "urllib.request"),
    ("urlparse", "urllib.parse"),
    ("whichdb", "dbm"),
    ("xmlrpclib", "xmlrpc.client"),
];

/// The names Python writes for the globals of Python 3 before protocol 3.
pub(crate) static REVERSE_NAME_MAPPING: &NameMapping = &[
    (("_functools", "reduce"), ("__builtin__", "reduce")),
    (("_socket", "socket"), ("socket", "_socketobject")),
    (
        ("builtins", "ArithmeticError"),
        ("exceptions", "ArithmeticError"),
    ),
    (
        ("builtins", "AssertionError"),
        ("exceptions", "AssertionError"),
    ),
    (
        ("builtins", "AttributeError"),
        ("exceptions", "AttributeError"),
    ),
    (
        ("builtins", "BaseException"),
        ("exceptions", "BaseException"),
    ),
    (("builtins", "BrokenPipeError"), ("exceptions", "OSError")),
    (("builtins", "BufferError"), ("exceptions", "BufferError")),
    (("builtins", "BytesWarning"), ("exceptions", "BytesWarning")),
    (("builtins", "ChildProcessError"), ("exceptions", "OSError")),
    (
        ("builtins", "ConnectionAbortedError"),
        ("exceptions", "OSError"),
    ),
    (("builtins", "ConnectionError"), ("exceptions", "OSError")),
    (
        ("builtins", "ConnectionRefusedError"),
        ("exceptions", "OSError"),
    ),
    (
        ("builtins", "ConnectionResetError"),
        ("exceptions", "OSError"),
    ),
    (
        ("builtins", "DeprecationWarning"),
        ("exceptions", "DeprecationWarning"),
    ),
    (("builtins", "EOFError"), ("exceptions", "EOFError")),
    (
        ("builtins", "EnvironmentError"),
        ("exceptions", "EnvironmentError"),
    ),
    (("builtins", "Exception"), ("exceptions", "Exception")),
    (("builtins", "FileExistsError"), ("exceptions", "OSError")),
    (("builtins", "FileNotFoundError"), ("exceptions", "OSError")),
    (
        ("builtins", "FloatingPointError"),
        ("exceptions", "FloatingPointError"),
    ),
    (
        ("builtins", "FutureWarning"),
        ("exceptions", "FutureWarning"),
    ),
    (
        ("builtins", "GeneratorExit"),
        ("exceptions", "GeneratorExit"),
    ),
    (("builtins", "IOError"), ("exceptions", "IOError")),
    (("builtins", "ImportError"), ("exceptions", "ImportError")),
    (
        ("builtins", "ImportWarning"),
        ("exceptions", "ImportWarning"),
    ),
    (
        ("builtins", "IndentationError"),
        ("exceptions", "IndentationError"),
    ),
    (("builtins", "IndexError"), ("exceptions", "IndexError")),
    (("builtins", "InterruptedError"), ("exceptions", "OSError")),
    (("builtins", "IsADirectoryError"), ("exceptions", "OSError")),
    (("builtins", "KeyError"), ("exceptions", "KeyError")),
    (
        ("builtins", "KeyboardInterrupt"),
        ("exceptions", "KeyboardInterrupt"),
    ),
    (("builtins", "LookupError"), ("exceptions", "LookupError")),
    (("builtins", "MemoryError"), ("exceptions", "MemoryError")),
    (
        ("builtins", "ModuleNotFoundError"),
        ("exceptions", "ImportError"),
    ),
    (("builtins", "NameError"), ("exceptions", "NameError")),
    (
        ("builtins", "NotADirectoryError"),
        ("exceptions", "OSError"),
    ),
    (
        ("builtins", "NotImplementedError"),
        ("exceptions", "NotImplementedError"),
    ),
    (("builtins", "OSError"), ("exceptions", "OSError")),
    (
        ("builtins", "OverflowError"),
        ("exceptions", "OverflowError"),
    ),
    (
        ("builtins", "PendingDeprecationWarning"),
        ("exceptions", "PendingDeprecationWarning"),
    ),
    (("builtins", "PermissionError"), ("exceptions", "OSError")),
    (
        ("builtins", "ProcessLookupError"),
        ("exceptions", "OSError"),
    ),
    (
        ("builtins", "ReferenceError"),
        ("exceptions", "ReferenceError"),
    ),
    (("builtins", "RuntimeError"), ("exceptions", "RuntimeError")),
    (
        ("builtins", "RuntimeWarning"),
        ("exceptions", "RuntimeWarning"),
    ),
    (
        ("builtins", "StopIteration"),
        ("exceptions", "StopIteration"),
    ),
    (("builtins", "SyntaxError"), ("exceptions", "SyntaxError")),
    (
        ("builtins", "SyntaxWarning"),
        ("exceptions", "SyntaxWarning"),
    ),
    (("builtins", "SystemError"), ("exceptions", "SystemError")),
    (("builtins", "SystemExit"), ("exceptions", "SystemExit")),
    (("builtins", "TabError"), ("exceptions", "TabError")),
    (("builtins", "TimeoutError"), ("exceptions", "OSError")),
    (("builtins", "TypeError"), ("exceptions", "TypeError")),
    (
        ("builtins", "UnboundLocalError"),
        ("exceptions", "UnboundLocalError"),
    ),
    (
        ("builtins", "UnicodeDecodeError"),
        ("exceptions", "UnicodeDecodeError"),
    ),
    (
        ("builtins", "UnicodeEncodeError"),
        ("exceptions", "UnicodeEncodeError"),
    ),
    (("builtins", "UnicodeError"), ("exceptions", "UnicodeError")),
    (
        ("builtins", "UnicodeTranslateError"),
        ("exceptions", "UnicodeTranslateError"),
    ),
    (
        ("builtins", "UnicodeWarning"),
        ("exceptions", "UnicodeWarning"),
    ),
    (("builtins", "UserWarning"), ("exceptions", "UserWarning")),
    (("builtins", "ValueError"), ("exceptions", "ValueError")),
    (("builtins", "Warning"), ("exceptions", "Warning")),
    (
        ("builtins", "ZeroDivisionError"),
        ("exceptions", "ZeroDivisionError"),
    ),
    (("builtins", "chr"), ("__builtin__", "unichr")),
    (("builtins", "filter"), ("itertools", "ifilter")),
    (("builtins", "int"), ("__builtin__", "long")),
    (("builtins", "map"), ("itertools", "imap")),
    (("builtins", "range"), ("__builtin__", "xrange")),
    (("builtins", "str"), ("__builtin__", "unicode")),
    (("builtins", "zip"), ("itertools", "izip")),
    (
        ("collections", "UserDict"),
        ("UserDict", "IterableUserDict"),
    ),
    (("collections", "UserList"), ("UserList", "UserList")),
    (("collections", "UserString"), ("UserString", "UserString")),
    (("dbm", "whichdb"), ("whichdb", "whichdb")),
    (("functools", "reduce"), ("__builtin__", "reduce")),
    (
        ("http.server", "CGIHTTPRequestHandler"),
        ("CGIHTTPServer", "CGIHTTPRequestHandler"),
    ),
    (
        ("http.server", "SimpleHTTPRequestHandler"),
        ("SimpleHTTPServer", "SimpleHTTPRequestHandler"),
    ),
    (("itertools", "filterfalse"), ("itertools", "ifilterfalse")),
    (("itertools", "zip_longest"), ("itertools", "izip_longest")),
    (
        ("multiprocessing.connection", "Connection"),
        ("_multiprocessing", "Connection"),
    ),
    (
        ("multiprocessing.context", "AuthenticationError"),
        ("multiprocessing", "AuthenticationError"),
    ),
    (
        ("multiprocessing.context", "BufferTooShort"),
        ("multiprocessing", "BufferTooShort"),
    ),
    (
        ("multiprocessing.context", "Process"),
        ("multiprocessing.process", "Process"),
    ),
    (
        ("multiprocessing.context", "ProcessError"),
        ("multiprocessing", "ProcessError"),
    ),
    (
        ("multiprocessing.context", "TimeoutError"),
        ("multiprocessing", "TimeoutError"),
    ),
    (
        ("multiprocessing.popen_fork", "Popen"),
        ("multiprocessing.forking", "Popen"),
    ),
    (("socket", "fromfd"), ("_socket", "fromfd")),
    (("sys", "intern"), ("__builtin__", "intern")),
    (
        ("tkinter.filedialog", "FileDialog"),
        ("FileDialog", "FileDialog"),
    ),
    (
        ("tkinter.filedialog", "LoadFileDialog"),
        ("FileDialog", "LoadFileDialog"),
    ),
    (
        ("tkinter.filedialog", "SaveFileDialog"),
        ("FileDialog", "SaveFileDialog"),
    ),
    (
        ("tkinter.simpledialog", "SimpleDialog"),
        ("SimpleDialog", "SimpleDialog"),
    ),
    (
        ("urllib.error", "ContentTooShortError"),
        ("urllib", "ContentTooShortError"),
    ),
    (("urllib.error", "HTTPError"), ("urllib2", "HTTPError")),
    (("urllib.error", "URLError"), ("urllib2", "URLError")),
    (("urllib.parse", "quote"), ("urllib", "quote")),
    (("urllib.parse", "quote_plus"), ("urllib", "quote_plus")),
    (("urllib.parse", "unquote"), ("urllib", "unquote")),
    (("urllib.parse", "unquote_plus"), ("urllib", "unquote_plus")),
    (("urllib.parse", "urlencode"), ("urllib", "urlencode")),
    (("urllib.request", "getproxies"), ("urllib", "getproxies")),
    (
        ("urllib.request", "pathname2url"),
        ("urllib", "pathname2url"),
    ),
    (
        ("urllib.request", "url2pathname"),
        ("urllib", "url2pathname"),
    ),
    (("urllib.request", "urlcleanup"), ("urllib", "urlcleanup")),
    (("urllib.request", "urlopen"), ("urllib", "urlopen")),
    (("urllib.request", "urlretrieve"), ("urllib", "urlretrieve")),
    (
        ("xmlrpc.server", "DocCGIXMLRPCRequestHandler"),
        ("DocXMLRPCServer", "DocCGIXMLRPCRequestHandler"),
    ),
    (
        ("xmlrpc.server", "DocXMLRPCRequestHandler"),
        ("DocXMLRPCServer", "DocXMLRPCRequestHandler"),
    ),
    (
        ("xmlrpc.server", "DocXMLRPCServer"),
        ("DocXMLRPCServer", "DocXMLRPCServer"),
    ),
    (
        ("xmlrpc.server", "ServerHTMLDoc"),
        ("DocXMLRPCServer", "ServerHTMLDoc"),
    ),
    (
        ("xmlrpc.server", "XMLRPCDocGenerator"),
        ("DocXMLRPCServer", "XMLRPCDocGenerator"),
    ),
];

/// The modules Python writes for the modules of Python 3 before protocol 3.
pub(crate) static REVERSE_IMPORT_MAPPING: &[(&str, &str)] = &[
    ("_bz2", "bz2"),
    ("_dbm", "dbm"),
    ("_dummy_thread", "dummy_thread"),
    ("_functools", "functools"),
    ("_gdbm", "gdbm"),
    ("_markupbase", "markupbase"),
    ("_pickle", "pickle"),
    ("_thread", "thread"),
    ("builtins", "__builtin__"),
    ("collections.abc", "_abcoll"),
    ("configparser", "ConfigParser"),
    ("copyreg", "copy_reg"),
    ("dbm", "anydbm"),
    ("dbm.bsd", "dbhash"),
    ("dbm.dumb", "dumbdbm"),
    ("dbm.gnu", "gdbm"),
    ("dbm.ndbm", "dbm"),
    ("html.entities", "htmlentitydefs"),
    ("html.parser", "HTMLParser"),
    ("http.client", "httplib"),
    ("http.cookiejar", "cookielib"),
    ("http.cookies", "Cookie"),
    ("http.server", "BaseHTTPServer"),
    ("queue", "Queue"),
    ("reprlib", "repr"),
    ("socketserver", "SocketServer"),
    ("subprocess", "commands"),
    ("test.support", "test.test_support"),
    ("tkinter", "Tkinter"),
    ("tkinter.colorchooser", "tkColorChooser"),
    ("tkinter.commondialog", "tkCommonDialog"),
    ("tkinter.constants", "Tkconstants"),
    ("tkinter.dialog", "Dialog"),
    ("tkinter.dnd", "Tkdnd"),
    ("tkinter.filedialog", "tkFileDialog"),
    ("tkinter.font", "tkFont"),
    ("tkinter.messagebox", "tkMessageBox"),
    ("tkinter.scrolledtext", "ScrolledText"),
    ("tkinter.simpledialog", "tkSimpleDialog"),
    ("tkinter.ttk", "ttk"),
    ("urllib.parse", "urlparse"),
    ("urllib.request", "urllib2"),
    ("urllib.robotparser", "robotparser"),
    ("winreg", "_winreg"),
    ("xmlrpc.client", "xmlrpclib"),
    ("xmlrpc.server", "SimpleXMLRPCServer"),
];

/// Returns the module and name (if it changes) Python 3 reads a global of
/// Python 2 as, `None` if neither changes.
pub(crate) fn fix_import(module: &str, name: &str) -> Option<(&'static str, Option<&'static str>)> {
    map(NAME_MAPPING, IMPORT_MAPPING, module, name)
}

/// Returns the module and name (if it changes) Python writes for a global
/// before protocol 3, `None` if neither changes.
pub(crate) fn reverse_fix_import(
    module: &str,
    name: &str,
) -> Option<(&'static str, Option<&'static str>)> {
    map(REVERSE_NAME_MAPPING, REVERSE_IMPORT_MAPPING, module, name)
}

fn map(
    names: &'static NameMapping,
    modules: &'static [(&'static str, &'static str)],
    module: &str,
    name: &str,
) -> Option<(&'static str, Option<&'static str>)> {
    if let Ok(idx) = names.binary_search_by(|(key, _)| key.cmp(&(module, name))) {
        let (module, name) = names[idx].1;
        return Some((module, Some(name)));
    }
    if let Ok(idx) = modules.binary_search_by(|(key, _)| (*key).cmp(module)) {
        return Some((modules[idx].1, None));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tables_are_sorted() {
        assert!(NAME_MAPPING.windows(2).all(|x| x[0].0 < x[1].0));
        assert!(IMPORT_MAPPING.windows(2).all(|x| x[0].0 < x[1].0));
        assert!(REVERSE_NAME_MAPPING.windows(2).all(|x| x[0].0 < x[1].0));
        assert!(REVERSE_IMPORT_MAPPING.windows(2).all(|x| x[0].0 < x[1].0));
    }

    #[test]
    fn test_fix_import() {
        assert_eq!(fix_import("__builtin__", "set"), Some(("builtins", None)));
        assert_eq!(
            fix_import("__builtin__", "unicode"),
            Some(("builtins", Some("str")))
        );
        assert_eq!(fix_import("app", "User"), None);
        assert_eq!(
            reverse_fix_import("builtins", "set"),
            Some(("__builtin__", None))
        );
    }
}
