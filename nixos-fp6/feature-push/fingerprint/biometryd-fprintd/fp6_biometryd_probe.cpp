// SPDX-License-Identifier: LGPL-3.0-only
//
// fp6-biometryd-probe: a client of com.ubports.biometryd.Service through
// biometryd's own D-Bus stub (the same stub the QML "Biometryd" module wraps
// for Lomiri System Settings and the greeter). Acts for the calling user only;
// biometryd enforces that.
//
//   fp6-biometryd-probe list | size | clear
//   fp6-biometryd-probe enroll [--cancel-after=SECONDS]
//   fp6-biometryd-probe identify [--cancel-after=SECONDS]
//   fp6-biometryd-probe remove ID
//
// The bus is the system bus, or DBUS_SYSTEM_BUS_ADDRESS (private-bus tests).

#include <biometry/application.h>
#include <biometry/device.h>
#include <biometry/identifier.h>
#include <biometry/operation.h>
#include <biometry/percent.h>
#include <biometry/progress.h>
#include <biometry/reason.h>
#include <biometry/service.h>
#include <biometry/template_store.h>
#include <biometry/user.h>
#include <biometry/void.h>
#include <biometry/dbus/service.h>
#include <biometry/devices/fingerprint_reader.h>
#include <biometry/variant.h>

#include <unistd.h>

#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <mutex>
#include <string>

namespace
{
struct Done
{
    std::mutex m;
    std::condition_variable cv;
    bool done = false;
    int rc = 1;
    void finish(int r)
    {
        std::lock_guard<std::mutex> l{m};
        done = true;
        rc = r;
        cv.notify_all();
    }
    // Returns false on timeout.
    bool wait(double seconds)
    {
        std::unique_lock<std::mutex> l{m};
        if (seconds <= 0)
        {
            cv.wait(l, [this] { return done; });
            return true;
        }
        return cv.wait_for(l, std::chrono::milliseconds(static_cast<long>(seconds * 1000)), [this] { return done; });
    }
};

std::string show(const biometry::User& u) { return std::to_string(u.id); }
std::string show(std::uint32_t v) { return std::to_string(v); }
std::string show(std::uint64_t v) { return std::to_string(v); }
std::string show(const biometry::Void&) { return "ok"; }
std::string show(const std::vector<biometry::TemplateStore::TemplateId>& v)
{
    std::string s = "[";
    for (std::size_t i = 0; i < v.size(); ++i) s += (i ? "," : "") + std::to_string(v[i]);
    return s + "]";
}

template <typename T>
struct Printer : public biometry::Operation<T>::Observer
{
    explicit Printer(Done& d) : d{d} {}
    void on_started() override { std::printf("started\n"); std::fflush(stdout); }
    void on_progress(const typename T::Progress& p) override
    {
        using Hints = biometry::devices::FingerprintReader::GuidedEnrollment::Hints;
        auto it = p.details.find(Hints::key_masks);
        if (it != p.details.end() && it->second.type() == biometry::Variant::Type::vector)
            std::printf("progress %.0f%% masks=%zu\n", 100.0 * *p.percent, it->second.vector().size());
        else
            std::printf("progress %.0f%%\n", 100.0 * *p.percent);
        std::fflush(stdout);
    }
    void on_canceled(const typename T::Reason& r) override { std::printf("canceled: %s\n", r.c_str()); d.finish(3); }
    void on_failed(const typename T::Error& e) override { std::printf("failed: %s\n", e.c_str()); d.finish(2); }
    void on_succeeded(const typename T::Result& r) override { std::printf("succeeded: %s\n", show(r).c_str()); d.finish(0); }
    Done& d;
};

template <typename T>
int run(const typename biometry::Operation<T>::Ptr& op, double cancel_after)
{
    Done d;
    op->start_with_observer(std::make_shared<Printer<T>>(d));
    if (!d.wait(cancel_after))
    {
        std::printf("cancelling after %.1f s\n", cancel_after);
        std::fflush(stdout);
        op->cancel();
        if (!d.wait(10)) { std::printf("no answer to cancel\n"); return 4; }
    }
    return d.rc;
}
}  // namespace

int main(int argc, char** argv)
{
    if (argc < 2)
    {
        std::fprintf(stderr, "usage: %s list|size|clear|enroll|identify|remove ID [--cancel-after=S]\n", argv[0]);
        return 64;
    }
    std::string cmd = argv[1];
    double cancel_after = 0;
    for (int i = 2; i < argc; ++i)
        if (std::strncmp(argv[i], "--cancel-after=", 15) == 0) cancel_after = std::atof(argv[i] + 15);

    std::shared_ptr<biometry::Device> dev;
    try
    {
        dev = biometry::dbus::Service::create_stub()->default_device();
    }
    catch (const std::exception& e)
    {
        std::printf("biometryd unavailable: %s\n", e.what());
        return 5;
    }
    biometry::Application app{biometry::Application::system()};
    biometry::User me{::getuid()};
    auto& ts = dev->template_store();
    using TS = biometry::TemplateStore;

    if (cmd == "list") return run<TS::List>(ts.list(app, me), cancel_after);
    if (cmd == "size") return run<TS::SizeQuery>(ts.size(app, me), cancel_after);
    if (cmd == "clear") return run<TS::Clearance>(ts.clear(app, me), cancel_after);
    if (cmd == "enroll") return run<TS::Enrollment>(ts.enroll(app, me), cancel_after);
    if (cmd == "remove" && argc >= 3) return run<TS::Removal>(ts.remove(app, me, std::strtoull(argv[2], nullptr, 10)), cancel_after);
    if (cmd == "identify")
        return run<biometry::Identification>(dev->identifier().identify_user(app, biometry::Reason{"fp6-biometryd-probe"}), cancel_after);
    std::fprintf(stderr, "unknown command\n");
    return 64;
}
