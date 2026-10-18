// JSON reader (runtime/net/json.h): recursive descent into a node tree.
#include "json.h"

#include <stdlib.h>
#include <string.h>

typedef struct { const char *p, *end; int depth; } Parser;

static void ws(Parser *s) {
    while (s->p < s->end && (*s->p == ' ' || *s->p == '\t' || *s->p == '\n' || *s->p == '\r')) s->p++;
}

static Json *node(JsonType t) {
    Json *j = calloc(1, sizeof *j);
    if (j) j->type = t;
    return j;
}

static void put_utf8(char **o, unsigned cp) {
    char *q = *o;
    if (cp < 0x80) *q++ = (char)cp;
    else if (cp < 0x800) *q++ = (char)(0xC0 | cp >> 6), *q++ = (char)(0x80 | (cp & 0x3F));
    else if (cp < 0x10000)
        *q++ = (char)(0xE0 | cp >> 12), *q++ = (char)(0x80 | ((cp >> 6) & 0x3F)), *q++ = (char)(0x80 | (cp & 0x3F));
    else
        *q++ = (char)(0xF0 | cp >> 18), *q++ = (char)(0x80 | ((cp >> 12) & 0x3F)),
        *q++ = (char)(0x80 | ((cp >> 6) & 0x3F)), *q++ = (char)(0x80 | (cp & 0x3F));
    *o = q;
}

static int hex4(const char *p, unsigned *v) {
    *v = 0;
    for (int i = 0; i < 4; i++) {
        char c = p[i];
        *v <<= 4;
        if (c >= '0' && c <= '9') *v |= (unsigned)(c - '0');
        else if (c >= 'a' && c <= 'f') *v |= (unsigned)(c - 'a' + 10);
        else if (c >= 'A' && c <= 'F') *v |= (unsigned)(c - 'A' + 10);
        else return 0;
    }
    return 1;
}

static char *string(Parser *s) {  // at the opening quote
    s->p++;
    const char *b = s->p;
    while (s->p < s->end && *s->p != '"') s->p += *s->p == '\\' ? 2 : 1;
    if (s->p >= s->end) return NULL;
    char *out = malloc((size_t)(s->p - b) * 2 + 1), *o = out;  // (\u escapes grow at most 6 -> 4 bytes)
    if (!out) return NULL;
    for (const char *q = b; q < s->p; q++) {
        if (*q != '\\') { *o++ = *q; continue; }
        q++;
        switch (*q) {
        case 'n': *o++ = '\n'; break;
        case 't': *o++ = '\t'; break;
        case 'r': *o++ = '\r'; break;
        case 'b': *o++ = '\b'; break;
        case 'f': *o++ = '\f'; break;
        case 'u': {
            unsigned cp;
            if (q + 4 >= s->p || !hex4(q + 1, &cp)) { free(out); return NULL; }
            q += 4;
            if (cp >= 0xD800 && cp < 0xDC00 && q + 6 < s->p && q[1] == '\\' && q[2] == 'u') {
                unsigned lo;
                if (hex4(q + 3, &lo) && lo >= 0xDC00 && lo < 0xE000) {
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                    q += 6;
                }
            }
            put_utf8(&o, cp);
            break;
        }
        default: *o++ = *q;  // \" \\ \/
        }
    }
    *o = 0;
    s->p++;
    return out;
}

static Json *value(Parser *s);

static Json *container(Parser *s, char close, JsonType t) {
    Json *j = node(t), **tail = j ? &j->child : NULL;
    if (!j) return NULL;
    s->p++;
    ws(s);
    if (s->p < s->end && *s->p == close) { s->p++; return j; }
    for (;;) {
        char *key = NULL;
        ws(s);
        if (t == JSON_OBJ) {
            if (s->p >= s->end || *s->p != '"' || !(key = string(s))) goto bad;
            ws(s);
            if (s->p >= s->end || *s->p != ':') { free(key); goto bad; }
            s->p++;
        }
        Json *v = value(s);
        if (!v) { free(key); goto bad; }
        v->key = key;
        *tail = v;
        tail = &v->next;
        ws(s);
        if (s->p < s->end && *s->p == ',') { s->p++; continue; }
        if (s->p < s->end && *s->p == close) { s->p++; return j; }
        goto bad;
    }
bad:
    json_free(j);
    return NULL;
}

static Json *value(Parser *s) {
    ws(s);
    if (s->p >= s->end || ++s->depth > 64) return NULL;
    Json *j = NULL;
    char c = *s->p;
    if (c == '{') j = container(s, '}', JSON_OBJ);
    else if (c == '[') j = container(s, ']', JSON_ARR);
    else if (c == '"') {
        char *str = string(s);
        if (str && (j = node(JSON_STR))) j->str = str;
        else free(str);
    } else if (!strncmp(s->p, "true", 4) && (j = node(JSON_BOOL))) j->num = 1, s->p += 4;
    else if (!strncmp(s->p, "false", 5) && (j = node(JSON_BOOL))) s->p += 5;
    else if (!strncmp(s->p, "null", 4) && (j = node(JSON_NULL))) s->p += 4;
    else if ((c == '-' || (c >= '0' && c <= '9')) && (j = node(JSON_NUM))) {
        char *e;
        j->num = strtod(s->p, &e);
        s->p = e;
    }
    s->depth--;
    return j;
}

Json *json_parse(const char *text, size_t len) {
    if (!text) return NULL;
    Parser s = {text, text + len, 0};
    Json *j = value(&s);
    ws(&s);
    if (j && s.p != s.end) { json_free(j); return NULL; }
    return j;
}

void json_free(Json *j) {
    while (j) {
        Json *n = j->next;
        json_free(j->child);
        free(j->key);
        free(j->str);
        free(j);
        j = n;
    }
}

Json *json_get(const Json *obj, const char *key) {
    if (!obj || obj->type != JSON_OBJ) return NULL;
    for (Json *c = obj->child; c; c = c->next)
        if (c->key && !strcmp(c->key, key)) return c;
    return NULL;
}

Json *json_at(const Json *arr, size_t i) {
    if (!arr || arr->type != JSON_ARR) return NULL;
    Json *c = arr->child;
    while (c && i--) c = c->next;
    return c;
}

size_t json_len(const Json *arr) {
    size_t n = 0;
    if (arr && (arr->type == JSON_ARR || arr->type == JSON_OBJ))
        for (Json *c = arr->child; c; c = c->next) n++;
    return n;
}

double json_num(const Json *j, double def) { return j && (j->type == JSON_NUM || j->type == JSON_BOOL) ? j->num : def; }
const char *json_str(const Json *j, const char *def) { return j && j->type == JSON_STR ? j->str : def; }
bool json_bool(const Json *j, bool def) { return j && (j->type == JSON_BOOL || j->type == JSON_NUM) ? j->num != 0 : def; }
