// Transporte BGR24 sin compresión. Mismo motor, pesos y muestreo que RIFE 20221029.
#include "rife.h"
#include "gpu.h"
#include <io.h>
#include <fcntl.h>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <chrono>
#include <condition_variable>
#include <deque>
#include <map>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

using Clock = std::chrono::steady_clock;
using Frame = std::shared_ptr<std::vector<unsigned char>>;
struct Job { int index; float time; Frame a, b; };

[[noreturn]] static void fail(const char* message) {
    fprintf(stderr, "Recorddy RIFE: %s\n", message);
    fflush(stderr);
    // Un error de tubería no debe dejar un lector o escritor esperando indefinidamente.
    std::_Exit(2);
}

int wmain(int argc, wchar_t** argv) {
    if (argc != 8) fail("modelo, ancho, alto, cuadros de entrada, muestreo de salida, trabajadores, cuadros emitidos");
    const int w = _wtoi(argv[2]), h = _wtoi(argv[3]);
    const int count = _wtoi(argv[4]), total = _wtoi(argv[5]), workers = _wtoi(argv[6]);
    const int emit = _wtoi(argv[7]);
    if (w < 1 || h < 1 || w > 16384 || h > 16384 || count < 1 || total < count || workers < 1 || workers > 2 || emit < 1 || emit > total)
        fail("argumentos inválidos");
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
    const size_t bytes = size_t(w) * h * 3;
    const auto begin = Clock::now();
    bool initialized = false;
    {
        std::unique_ptr<RIFE> rife;
        std::once_flag initialize;
        std::mutex mutex;
        std::condition_variable changed;
        std::deque<Job> jobs;
        std::map<int, Frame> results;
        int pending = 0;
        bool done = false;
        std::vector<std::thread> pool;
        for (int worker = 0; worker < workers; ++worker) pool.emplace_back([&] {
            for (;;) {
                Job job;
                {
                    std::unique_lock<std::mutex> lock(mutex);
                    changed.wait(lock, [&] { return done || !jobs.empty(); });
                    if (jobs.empty()) return;
                    job = std::move(jobs.front()); jobs.pop_front();
                }
                Frame output;
                if (job.time == 0.f || (job.time != 1.f && std::memcmp(job.a->data(), job.b->data(), bytes) == 0)) output = job.a;
                else if (job.time == 1.f) output = job.b;
                else {
                    // Los tramos constantes no necesitan crear dispositivos ni cargar pesos.
                    std::call_once(initialize, [&] {
                        ncnn::create_gpu_instance();
                        const int gpu = ncnn::get_default_gpu_index();
                        if (gpu < 0 || gpu >= ncnn::get_gpu_count()) fail("no hay GPU Vulkan utilizable");
                        rife = std::make_unique<RIFE>(gpu, false, false, false, 1, false, true);
                        if (rife->load(argv[1]) != 0) fail("no se pudo cargar el modelo v4.6");
                        initialized = true;
                    });
                    output = std::make_shared<std::vector<unsigned char>>(bytes);
                    ncnn::Mat a(w, h, job.a->data(), size_t(3), 3);
                    ncnn::Mat b(w, h, job.b->data(), size_t(3), 3);
                    ncnn::Mat out(w, h, output->data(), size_t(3), 3);
                    if (rife->process(a, b, job.time, out) != 0) fail("falló la interpolación");
                }
                {
                    std::lock_guard<std::mutex> lock(mutex);
                    results.emplace(job.index, std::move(output));
                }
                changed.notify_all();
            }
        });
        std::thread writer([&] {
            for (int i = 0; i < emit; ++i) {
                Frame output;
                {
                    std::unique_lock<std::mutex> lock(mutex);
                    changed.wait(lock, [&] { return results.count(i) != 0; });
                    output = std::move(results.at(i)); results.erase(i);
                }
                if (fwrite(output->data(), 1, bytes, stdout) != bytes) fail("se cerró la salida");
                {
                    std::lock_guard<std::mutex> lock(mutex);
                    --pending;
                }
                changed.notify_all();
            }
            if (fflush(stdout)) fail("no se pudo completar la salida");
        });
        auto read = [&] {
            auto frame = std::make_shared<std::vector<unsigned char>>(bytes);
            if (fread(frame->data(), 1, bytes, stdin) != bytes) fail("entrada incompleta");
            return frame;
        };
        Frame a = read(), b = count > 1 ? read() : a;
        int pair = 0;
        const double scale = double(count) / total;
        for (int i = 0; i < emit; ++i) {
            float time = float(i * scale);
            int sample = int(time);
            time -= sample;
            if (sample >= count - 1) { sample = count > 1 ? count - 2 : 0; time = 1.f; }
            while (pair < sample) { a = std::move(b); b = read(); ++pair; }
            std::unique_lock<std::mutex> lock(mutex);
            // El crédito se libera al escribir: también limita resultados fuera de orden.
            changed.wait(lock, [&] { return pending < 4; });
            ++pending;
            jobs.push_back({i, time, a, b});
            lock.unlock(); changed.notify_all();
        }
        {
            std::lock_guard<std::mutex> lock(mutex);
            done = true;
        }
        changed.notify_all();
        for (auto& thread : pool) thread.join();
        writer.join();
    }
    if (initialized) ncnn::destroy_gpu_instance();
    fprintf(stderr, "Recorddy RIFE: %d cuadros, %.3f s\n", emit,
        std::chrono::duration<double>(Clock::now() - begin).count());
    return 0;
}
