// transcribe-server: OpenFlow's speech server for meetings. A small HTTP wrapper around transcribe.cpp
// (https://github.com/handy-computer/transcribe.cpp, pinned in PINNED_COMMIT) that holds one GGUF model hot on
// Metal. It replaces Homebrew's whisper-server: about half the memory and twice the speed on Apple silicon.
//
// It speaks the subset of whisper-server's API that OpenFlow uses, so the Rust client barely changed:
//
//   GET  /            200 "ok" once the model is loaded (readiness)
//   POST /inference   multipart: file (16 kHz mono s16 WAV), optional language ("auto"), prompt, temperature,
//                     temperature_inc. Returns
//                     {"text", "language", "truncated", "segments": [{"text", "no_speech_prob", "avg_logprob",
//                      "compression_ratio"}]}
//
// The segments carry Whisper's per-chunk decode trace (transcribe.cpp >= 0.2), which OpenFlow's hallucination
// filter reads. Every request must carry `Authorization: Bearer <token>` (token from OPENFLOW_STT_TOKEN, set by
// the app per launch): the port is on 127.0.0.1, but any web page can POST to localhost.
//
// Adapted from Meeting-Assistant's transcribe-server (MIT, Copyright (c) 2026 Rahul Khatri); see NOTICE.md.

#include "transcribe.h"
#include "transcribe/whisper.h"
#include "httplib.h"

#include <cerrno>
#include <chrono>
#include <cmath>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <string>
#include <thread>
#include <vector>
#include <unistd.h>

static transcribe_model *g_model = nullptr;
static transcribe_session *g_ctx = nullptr;
// transcribe.cpp does not allow concurrent compute on one session: requests are served one at a time.
static std::mutex g_compute_mtx;

// Scans for the "data" chunk of a 16 kHz mono s16 WAV and expands it to float PCM.
static bool parse_wav16(const std::string &b, std::vector<float> &out) {
    if (b.size() < 12 || b.compare(0, 4, "RIFF") != 0 || b.compare(8, 4, "WAVE") != 0) return false;
    size_t i = 12;
    while (i + 8 <= b.size()) {
        uint32_t sz = (uint8_t)b[i + 4] | ((uint8_t)b[i + 5] << 8) | ((uint8_t)b[i + 6] << 16) |
                      ((uint32_t)(uint8_t)b[i + 7] << 24);
        if (b.compare(i, 4, "data") == 0) {
            size_t start = i + 8, n = std::min((size_t)sz, b.size() - start), ns = n / 2;
            out.resize(ns);
            for (size_t k = 0; k < ns; k++) {
                int16_t s = (int16_t)((uint8_t)b[start + 2 * k] | ((uint16_t)(uint8_t)b[start + 2 * k + 1] << 8));
                out[k] = s / 32768.0f;
            }
            return true;
        }
        i += 8 + sz + (sz & 1);
    }
    return false;
}

// JSON string body. Invalid UTF-8 (a multibyte character cut by a token boundary) becomes U+FFFD so the reply
// always parses.
static std::string json_escape(const std::string &s) {
    std::string o;
    for (size_t i = 0; i < s.size();) {
        const unsigned char c = static_cast<unsigned char>(s[i]);
        if (c >= 0x80) {
            size_t width = 0;
            if (c >= 0xC2 && c <= 0xDF) width = 2;
            else if (c >= 0xE0 && c <= 0xEF) width = 3;
            else if (c >= 0xF0 && c <= 0xF4) width = 4;
            bool valid = width != 0 && i + width <= s.size();
            for (size_t j = 1; valid && j < width; ++j) {
                const unsigned char cont = static_cast<unsigned char>(s[i + j]);
                valid = cont >= 0x80 && cont <= 0xBF;
            }
            if (valid && width == 3) {
                const unsigned char second = static_cast<unsigned char>(s[i + 1]);
                valid = !((c == 0xE0 && second < 0xA0) || (c == 0xED && second > 0x9F));
            } else if (valid && width == 4) {
                const unsigned char second = static_cast<unsigned char>(s[i + 1]);
                valid = !((c == 0xF0 && second < 0x90) || (c == 0xF4 && second > 0x8F));
            }
            if (valid) {
                o.append(s, i, width);
                i += width;
            } else {
                o += "\xEF\xBF\xBD";
                ++i;
            }
            continue;
        }
        if (c == '"' || c == '\\') {
            o += '\\';
            o += static_cast<char>(c);
        } else if (c == '\n' || c == '\r' || c == '\t') {
            o += ' ';
        } else if (c >= 0x20) {
            o += static_cast<char>(c);
        }
        ++i;
    }
    return o;
}

static std::string num(double v) {
    if (!std::isfinite(v)) return "0";
    char buf[32];
    snprintf(buf, sizeof buf, "%.4f", v);
    return buf;
}

static std::string trim(std::string s) {
    size_t b = s.find_first_not_of(' ');
    if (b == std::string::npos) return "";
    size_t e = s.find_last_not_of(' ');
    return s.substr(b, e - b + 1);
}

struct Segment {
    std::string text;
    double no_speech_prob = 0, avg_logprob = 0, compression_ratio = 1;
};

// One entry per Whisper decode chunk (<= 30 s; an OpenFlow utterance is one chunk), with the chunk's text and
// its trace. Other model families have no trace: their transcript segments come back with confidence from the
// token probabilities.
static std::vector<Segment> result_segments() {
    std::vector<Segment> out;
    const int n_seg = transcribe_n_segments(g_ctx);
    std::vector<transcribe_segment> segs(n_seg);
    for (int i = 0; i < n_seg; i++) {
        transcribe_segment_init(&segs[i]);
        transcribe_get_segment(g_ctx, i, &segs[i]);
    }
    const int n_chunks = transcribe_get_whisper_chunk_count(g_ctx);
    if (n_chunks > 0) {
        for (int c = 0; c < n_chunks; c++) {
            transcribe_whisper_chunk_trace tr;
            transcribe_whisper_chunk_trace_init(&tr);
            if (transcribe_get_whisper_chunk_trace(g_ctx, c, &tr) != TRANSCRIBE_OK) continue;
            Segment s;
            s.no_speech_prob = tr.no_speech_prob;
            s.avg_logprob = tr.avg_logprob;
            s.compression_ratio = tr.compression_ratio;
            for (const auto &seg : segs) {
                const bool last = c == n_chunks - 1;
                if (seg.text && seg.t0_ms >= tr.t0_ms && (seg.t0_ms < tr.t1_ms || last)) s.text += seg.text;
            }
            if (tr.no_speech_triggered) s.text.clear();
            out.push_back(s);
        }
        return out;
    }
    for (const auto &seg : segs) {
        Segment s;
        s.text = seg.text ? seg.text : "";
        double sum = 0;
        int n = 0;
        for (int t = seg.first_token; t < seg.first_token + seg.n_tokens; t++) {
            transcribe_token tok;
            transcribe_token_init(&tok);
            if (transcribe_get_token(g_ctx, t, &tok) == TRANSCRIBE_OK && tok.p > 0) {
                sum += std::log(tok.p);
                n++;
            }
        }
        s.avg_logprob = n ? sum / n : 0;
        out.push_back(s);
    }
    return out;
}

// Exits when the app that started us is gone (crash or force quit), releasing the port and ~1 GB of model.
static void start_parent_watchdog() {
    const char *raw = getenv("OPENFLOW_PARENT_PID");
    if (!raw || !*raw) return;
    char *end = nullptr;
    long parsed = strtol(raw, &end, 10);
    if (!end || *end != '\0' || parsed <= 1) return;
    const pid_t parent = static_cast<pid_t>(parsed);
    std::thread([parent] {
        for (;;) {
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            if (kill(parent, 0) == -1 && errno == ESRCH) std::_Exit(0);
        }
    }).detach();
}

static bool selftest() {
    if (json_escape("है \"ready\"\n") != "है \\\"ready\\\" ") return false;
    std::string cut = "partial";
    cut.push_back(static_cast<char>(0xE0));
    if (json_escape(cut) != "partial\xEF\xBF\xBD") return false;
    std::string wav("RIFF\0\0\0\0WAVEdata\x06\0\0\0\x00\x80\x00\x00\xFF\x7F", 26);
    std::vector<float> pcm;
    if (!parse_wav16(wav, pcm) || pcm.size() != 3 || pcm[0] != -1.0f || pcm[1] != 0.0f || pcm[2] < 0.999f) return false;
    return trim("  hi there ") == "hi there" && num(NAN) == "0";
}

int main(int argc, char **argv) {
    if (argc == 2 && std::strcmp(argv[1], "--selftest") == 0) {
        const bool ok = selftest();
        fprintf(stderr, "transcribe-server self-test %s\n", ok ? "passed" : "failed");
        return ok ? 0 : 1;
    }
    signal(SIGPIPE, SIG_IGN);
    start_parent_watchdog();
    const char *token_env = getenv("OPENFLOW_STT_TOKEN");
    const std::string token = token_env ? token_env : "";
    std::string model_path, host = "127.0.0.1";
    int port = 8768;
    for (int i = 1; i < argc; i++) {
        std::string a = argv[i];
        if (a == "-m" && i + 1 < argc) model_path = argv[++i];
        else if (a == "--host" && i + 1 < argc) host = argv[++i];
        else if (a == "--port" && i + 1 < argc) port = atoi(argv[++i]);
    }
    if (model_path.empty() || token.size() < 16) {
        fprintf(stderr, "usage: OPENFLOW_STT_TOKEN=<16+ chars> transcribe-server -m model.gguf [--host H] [--port P]\n");
        return 2;
    }

    transcribe_init_backends_default();
    transcribe_model_load_params mp;
    transcribe_model_load_params_init(&mp);
    if (transcribe_model_load_file(model_path.c_str(), &mp, &g_model) != TRANSCRIBE_OK) {
        fprintf(stderr, "transcribe-server: model load failed: %s\n", model_path.c_str());
        return 1;
    }
    transcribe_session_params sp;
    transcribe_session_params_init(&sp);
    if (transcribe_session_init(g_model, &sp, &g_ctx) != TRANSCRIBE_OK) {
        fprintf(stderr, "transcribe-server: session init failed\n");
        return 1;
    }
    const bool whisper = transcribe_model_accepts_ext_kind(g_model, TRANSCRIBE_EXT_SLOT_RUN, TRANSCRIBE_EXT_KIND_WHISPER_RUN);

    httplib::Server srv;
    srv.set_payload_max_length(64ULL * 1024ULL * 1024ULL);
    auto authorized = [token](const httplib::Request &req, httplib::Response &res) {
        if (req.get_header_value("Authorization") != "Bearer " + token) {
            res.status = 401;
            res.set_content("unauthorized", "text/plain");
            return false;
        }
        return true;
    };
    srv.Get("/", [authorized](const httplib::Request &req, httplib::Response &res) {
        if (!authorized(req, res)) return;
        res.set_content("ok", "text/plain");
    });
    srv.Post("/inference", [authorized, whisper](const httplib::Request &req, httplib::Response &res) {
        if (!authorized(req, res)) return;
        auto field = [&req](const char *name, const std::string &fallback) {
            return req.has_file(name) ? req.get_file_value(name).content : fallback;
        };
        std::vector<float> pcm;
        if (!parse_wav16(field("file", ""), pcm) || pcm.empty()) {
            res.status = 400;
            res.set_content("{\"error\":\"16 kHz mono s16 WAV required\"}", "application/json");
            return;
        }
        const std::string lang = field("language", "auto");
        const std::string prompt = field("prompt", "");
        const float temperature = std::strtof(field("temperature", "0").c_str(), nullptr);
        const float temperature_inc = std::strtof(field("temperature_inc", "0").c_str(), nullptr);

        std::lock_guard<std::mutex> lock(g_compute_mtx);
        transcribe_run_params rp;
        transcribe_run_params_init(&rp);
        if (!lang.empty() && lang != "auto") rp.language = lang.c_str();
        transcribe_whisper_run_ext wx;
        transcribe_whisper_run_ext_init(&wx);
        // Temperature fallback is where Whisper's fluent inventions on noise come from; OpenFlow sends 0.
        wx.temperature = temperature;
        wx.temperature_inc = temperature_inc;
        wx.no_speech_thold = 0.6f;
        if (!prompt.empty()) wx.initial_prompt = prompt.c_str();
        if (whisper) rp.family = &wx.ext;

        const transcribe_status st = transcribe_run(g_ctx, pcm.data(), (int)pcm.size(), &rp);
        const bool truncated = st == TRANSCRIBE_ERR_OUTPUT_TRUNCATED;
        if (st != TRANSCRIBE_OK && !truncated) {
            fprintf(stderr, "transcribe-server: inference failed: %s\n", transcribe_status_string(st));
            res.status = 500;
            res.set_content(std::string("{\"error\":\"") + json_escape(transcribe_status_string(st)) + "\"}",
                            "application/json");
            return;
        }
        const char *t = transcribe_full_text(g_ctx);
        const char *l = transcribe_detected_language(g_ctx);
        std::string body = "{\"text\":\"" + json_escape(trim(t ? t : "")) + "\",\"language\":\"" +
                           json_escape(l && *l ? l : (lang == "auto" ? "" : lang)) + "\",\"truncated\":" +
                           (truncated ? "true" : "false") + ",\"segments\":[";
        bool first = true;
        for (const auto &s : result_segments()) {
            body += (first ? "" : ",");
            body += "{\"text\":\"" + json_escape(s.text) + "\",\"no_speech_prob\":" + num(s.no_speech_prob) +
                    ",\"avg_logprob\":" + num(s.avg_logprob) + ",\"compression_ratio\":" + num(s.compression_ratio) +
                    "}";
            first = false;
        }
        body += "]}";
        res.set_content(body, "application/json");
    });

    fprintf(stderr, "transcribe-server ready on %s:%d (%s)\n", host.c_str(), port, transcribe_version());
    if (!srv.listen(host.c_str(), port)) {
        fprintf(stderr, "transcribe-server: cannot listen on %s:%d\n", host.c_str(), port);
        return 1;
    }
    transcribe_session_free(g_ctx);
    transcribe_model_free(g_model);
    return 0;
}
