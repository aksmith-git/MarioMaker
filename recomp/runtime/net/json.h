// A small JSON reader (a tree of nodes) for answers from a mod's server. Lookups on a missing key / index or on a
// node of another type return NULL or the default, so callers can chain them without checks.
#pragma once
#include <stdbool.h>
#include <stddef.h>

typedef enum { JSON_NULL, JSON_BOOL, JSON_NUM, JSON_STR, JSON_ARR, JSON_OBJ } JsonType;

typedef struct Json {
    JsonType type;
    char *key;           // member name (in an object)
    char *str;           // JSON_STR (UTF-8, unescaped)
    double num;          // JSON_NUM, JSON_BOOL (0 / 1)
    struct Json *child;  // first element / member
    struct Json *next;   // next sibling
} Json;

Json *json_parse(const char *text, size_t len);  // NULL on a syntax error
void json_free(Json *j);
Json *json_get(const Json *obj, const char *key);
Json *json_at(const Json *arr, size_t i);
size_t json_len(const Json *arr);
double json_num(const Json *j, double def);
const char *json_str(const Json *j, const char *def);
bool json_bool(const Json *j, bool def);
