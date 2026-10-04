/* Prints how inih parses files, for scripts/ini/references.
 *
 * Built with INI_HANDLER_LINENO=1 and INI_CALL_HANDLER_ON_NEW_SECTION=1.
 * For every file named on stdin (one per line) it prints a JSON line:
 *
 *   {"file": "...", "events": [["section", line, "name"],
 *                              ["value", line, "section", "name", "value"]],
 *    "error_line": null}
 *
 * Strings are hex encoded as {"hex": "..."}, scripts/ini/references/write.py
 * turns them back into text.  A value is null for a name without a value
 * (INI_ALLOW_NO_VALUE).  error_line is the line of the first error, inih
 * keeps parsing after errors.
 */

#include <stdio.h>
#include <string.h>

#include "ini.h"

static int first;

static void print_hex(const char *s) {
    if (!s) {
        fputs("null", stdout);
        return;
    }
    fputs("{\"hex\":\"", stdout);
    for (; *s; s++)
        printf("%02x", (unsigned char)*s);
    fputs("\"}", stdout);
}

static int handler(void *user, const char *section, const char *name,
                   const char *value, int lineno) {
    (void)user;
    fputs(first ? "" : ",", stdout);
    first = 0;
    if (!name) {
        printf("[\"section\",%d,", lineno);
        print_hex(section);
    } else {
        printf("[\"value\",%d,", lineno);
        print_hex(section);
        fputc(',', stdout);
        print_hex(name);
        fputc(',', stdout);
        print_hex(value);
    }
    fputc(']', stdout);
    return 1;
}

int main(void) {
    char path[4096];
    while (fgets(path, sizeof(path), stdin)) {
        path[strcspn(path, "\n")] = '\0';
        if (!*path)
            continue;
        fputs("{\"file\":", stdout);
        print_hex(path);
        fputs(",\"events\":[", stdout);
        first = 1;
        int rv = ini_parse(path, handler, NULL);
        if (rv < 0) {
            fprintf(stderr, "%s: ini_parse failed with %d\n", path, rv);
            return 1;
        }
        printf("],\"error_line\":");
        if (rv)
            printf("%d", rv);
        else
            fputs("null", stdout);
        fputs("}\n", stdout);
        fflush(stdout);
    }
    return 0;
}
