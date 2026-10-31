// A small JSON reader for the state document the core hands back (bb_app_state_json). Read-only, no
// surprises: objects keep their order, missing members read as null, numbers are doubles.
#pragma once
#include <cstdint>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

namespace bb::json {

class Value {
public:
    enum class Type { Null, Bool, Number, String, Array, Object };

    Type type = Type::Null;
    bool b = false;
    double n = 0;
    std::string s;
    std::vector<Value> a;
    std::vector<std::pair<std::string, Value>> o;

    bool is_null() const { return type == Type::Null; }
    bool is_array() const { return type == Type::Array; }
    bool is_object() const { return type == Type::Object; }
    const Value& operator[](const std::string& key) const {
        static const Value none;
        for (const auto& kv : o) {
            if (kv.first == key) return kv.second;
        }
        return none;
    }
    const Value& operator[](size_t i) const {
        static const Value none;
        return i < a.size() ? a[i] : none;
    }
    size_t size() const { return type == Type::Array ? a.size() : o.size(); }
    // Typed reads with a fallback for null or the wrong type.
    std::string str(const std::string& fallback = {}) const { return type == Type::String ? s : fallback; }
    double num(double fallback = 0) const { return type == Type::Number ? n : fallback; }
    int64_t i64(int64_t fallback = 0) const { return type == Type::Number ? static_cast<int64_t>(n) : fallback; }
    bool boolean(bool fallback = false) const { return type == Type::Bool ? b : fallback; }
};

namespace detail {

class Parser {
public:
    explicit Parser(const std::string& t) : t_(t) {}
    Value parse() {
        Value v = value();
        ws();
        if (i_ != t_.size()) fail("trailing text");
        return v;
    }

private:
    [[noreturn]] void fail(const char* what) const {
        throw std::runtime_error(std::string("json: ") + what + " at " + std::to_string(i_));
    }
    void ws() {
        while (i_ < t_.size() && (t_[i_] == ' ' || t_[i_] == '\n' || t_[i_] == '\r' || t_[i_] == '\t')) ++i_;
    }
    char peek() {
        ws();
        if (i_ >= t_.size()) fail("unexpected end");
        return t_[i_];
    }
    void expect(char c) {
        if (peek() != c) fail("unexpected character");
        ++i_;
    }
    bool literal(const char* word) {
        const std::string w = word;
        if (t_.compare(i_, w.size(), w) == 0) {
            i_ += w.size();
            return true;
        }
        return false;
    }
    static void append_utf8(std::string& out, uint32_t cp) {
        if (cp < 0x80) {
            out += static_cast<char>(cp);
        } else if (cp < 0x800) {
            out += static_cast<char>(0xC0 | (cp >> 6));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        } else if (cp < 0x10000) {
            out += static_cast<char>(0xE0 | (cp >> 12));
            out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        } else {
            out += static_cast<char>(0xF0 | (cp >> 18));
            out += static_cast<char>(0x80 | ((cp >> 12) & 0x3F));
            out += static_cast<char>(0x80 | ((cp >> 6) & 0x3F));
            out += static_cast<char>(0x80 | (cp & 0x3F));
        }
    }
    uint32_t hex4() {
        if (i_ + 4 > t_.size()) fail("bad escape");
        const uint32_t v = static_cast<uint32_t>(std::strtoul(t_.substr(i_, 4).c_str(), nullptr, 16));
        i_ += 4;
        return v;
    }
    std::string string() {
        expect('"');
        std::string out;
        while (i_ < t_.size() && t_[i_] != '"') {
            char c = t_[i_++];
            if (c != '\\') {
                out += c;
                continue;
            }
            if (i_ >= t_.size()) fail("bad escape");
            c = t_[i_++];
            switch (c) {
                case 'n': out += '\n'; break;
                case 'r': out += '\r'; break;
                case 't': out += '\t'; break;
                case 'b': out += '\b'; break;
                case 'f': out += '\f'; break;
                case 'u': {
                    uint32_t cp = hex4();
                    if (cp >= 0xD800 && cp < 0xDC00 && t_.compare(i_, 2, "\\u") == 0) {
                        i_ += 2;
                        const uint32_t lo = hex4();
                        cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                    }
                    append_utf8(out, cp);
                    break;
                }
                default: out += c; break;
            }
        }
        if (i_ >= t_.size()) fail("unterminated string");
        ++i_;
        return out;
    }
    Value value() {
        Value v;
        const char c = peek();
        if (c == '{') {
            ++i_;
            v.type = Value::Type::Object;
            if (peek() == '}') {
                ++i_;
                return v;
            }
            for (;;) {
                std::string key = string();
                expect(':');
                v.o.emplace_back(std::move(key), value());
                if (peek() == ',') {
                    ++i_;
                    continue;
                }
                expect('}');
                return v;
            }
        }
        if (c == '[') {
            ++i_;
            v.type = Value::Type::Array;
            if (peek() == ']') {
                ++i_;
                return v;
            }
            for (;;) {
                v.a.push_back(value());
                if (peek() == ',') {
                    ++i_;
                    continue;
                }
                expect(']');
                return v;
            }
        }
        if (c == '"') {
            v.type = Value::Type::String;
            v.s = string();
            return v;
        }
        if (literal("true")) {
            v.type = Value::Type::Bool;
            v.b = true;
            return v;
        }
        if (literal("false")) {
            v.type = Value::Type::Bool;
            return v;
        }
        if (literal("null")) return v;
        char* end = nullptr;
        v.n = std::strtod(t_.c_str() + i_, &end);
        if (end == t_.c_str() + i_) fail("unexpected character");
        i_ = static_cast<size_t>(end - t_.c_str());
        v.type = Value::Type::Number;
        return v;
    }

    const std::string& t_;
    size_t i_ = 0;
};

}  // namespace detail

inline Value parse(const std::string& text) { return detail::Parser(text).parse(); }

}  // namespace bb::json
