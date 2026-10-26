// SPDX-License-Identifier: LGPL-3.0-only
//
// fp6-biometryd-fprintd: a biometryd device plugin that serves biometryd's
// Device / TemplateStore / Identifier / Verifier on top of the fprintd D-Bus API
// (net.reactivated.Fprint, here provided by fingerprintd on the FP6).
//
// biometryd keeps its own D-Bus skeleton, request verification (caller uid ==
// template owner) and wire format; Lomiri System Settings (Fingerprint ID) and
// the Lomiri greeter talk to it unchanged. This plugin only translates:
//
//   TemplateStore.size/list  -> ListEnrolledFingers(user)
//   TemplateStore.enroll     -> Claim(user), EnrollStart(<first free finger>),
//                               EnrollStatus signals -> progress/result, EnrollStop, Release
//   TemplateStore.remove(id) -> Claim(user), DeleteEnrolledFinger(<finger of id>), Release
//   TemplateStore.clear      -> DeleteEnrolledFingers(user)
//   Identifier.identify_user -> owner of the active seat0 session; Claim, VerifyStart("any"),
//                               VerifyStatus -> User{uid}, VerifyStop, Release
//   Verifier.verify_user     -> the same for the given user
//
// Template ids are the fprintd finger slots, 1..10 (left-thumb .. right-little-finger),
// stable across restarts. No biometric data passes through this plugin; it never
// touches the sensor, the TEE or any storage itself.
//
// The plugin talks to the REAL system bus (unix:path=/run/dbus/system_bus_socket)
// even if biometryd itself was pointed at another bus via DBUS_SYSTEM_BUS_ADDRESS
// (private-bus testing). FP6_BIOMETRYD_FPRINT_BUS overrides that address.

#include <biometry/application.h>
#include <biometry/device.h>
#include <biometry/identifier.h>
#include <biometry/operation.h>
#include <biometry/percent.h>
#include <biometry/progress.h>
#include <biometry/reason.h>
#include <biometry/template_store.h>
#include <biometry/user.h>
#include <biometry/verifier.h>
#include <biometry/void.h>
#include <biometry/devices/plugin/interface.h>
#include <biometry/devices/fingerprint_reader.h>
#include <biometry/geometry.h>
#include <biometry/variant.h>

#include <systemd/sd-bus.h>

#include <pwd.h>
#include <unistd.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cstdarg>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <functional>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

namespace
{
constexpr const char* kService = "net.reactivated.Fprint";
constexpr const char* kManagerPath = "/net/reactivated/Fprint/Manager";
constexpr const char* kManagerIface = "net.reactivated.Fprint.Manager";
constexpr const char* kDeviceIface = "net.reactivated.Fprint.Device";
constexpr const char* kErrPrefix = "net.reactivated.Fprint.Error.";

// fprintd finger names; template id = index + 1.
constexpr std::array<const char*, 10> kFingers{
    "left-thumb", "left-index-finger", "left-middle-finger", "left-ring-finger", "left-little-finger",
    "right-thumb", "right-index-finger", "right-middle-finger", "right-ring-finger", "right-little-finger"};
// The FP6 sensor is in the side power key: thumbs and index fingers first.
constexpr std::array<int, 10> kEnrolOrder{5, 0, 6, 1, 7, 2, 8, 3, 9, 4};

void log(const char* fmt, ...)
{
    std::va_list ap;
    va_start(ap, fmt);
    std::fputs("fp6-biometryd-fprintd: ", stderr);
    std::vfprintf(stderr, fmt, ap);
    std::fputc('\n', stderr);
    std::fflush(stderr);
    va_end(ap);
}

int finger_index(const std::string& name)
{
    for (std::size_t i = 0; i < kFingers.size(); ++i)
        if (name == kFingers[i]) return static_cast<int>(i);
    return -1;
}

std::string user_name(uid_t uid)
{
    std::vector<char> buf(16384);
    passwd pw{}, *res = nullptr;
    if (::getpwuid_r(uid, &pw, buf.data(), buf.size(), &res) == 0 && res) return pw.pw_name;
    return {};
}

// Error short name ("AlreadyInUse") of an fprintd error, or the full name.
std::string short_error(const sd_bus_error& e)
{
    if (!e.name) return "unknown";
    std::string n = e.name;
    if (n.rfind(kErrPrefix, 0) == 0) return n.substr(std::strlen(kErrPrefix));
    return n;
}

// One private connection per operation: the fprintd claim belongs to this
// connection and is dropped by the daemon if the connection goes away.
class Fprint
{
public:
    struct Status
    {
        std::string result;
        bool done = false;
        bool fresh = false;
    };

    ~Fprint()
    {
        if (enroll_slot) sd_bus_slot_unref(enroll_slot);
        if (verify_slot) sd_bus_slot_unref(verify_slot);
        if (owner_slot) sd_bus_slot_unref(owner_slot);
        if (bus) sd_bus_flush_close_unref(bus);
    }

    // Returns an empty string on success, else an error description.
    std::string open()
    {
        const char* addr = std::getenv("FP6_BIOMETRYD_FPRINT_BUS");
        if (!addr || !*addr) addr = "unix:path=/run/dbus/system_bus_socket";
        int r = sd_bus_new(&bus);
        if (r >= 0) r = sd_bus_set_address(bus, addr);
        if (r >= 0) r = sd_bus_set_bus_client(bus, 1);
        if (r >= 0) r = sd_bus_start(bus);
        if (r < 0) return std::string("cannot connect to the system bus: ") + std::strerror(-r);

        sd_bus_error err = SD_BUS_ERROR_NULL;
        sd_bus_message* reply = nullptr;
        r = sd_bus_call_method(bus, kService, kManagerPath, kManagerIface, "GetDefaultDevice", &err, &reply, "");
        if (r < 0)
        {
            std::string e = std::string("fprintd unavailable: ") + (err.message ? err.message : std::strerror(-r));
            sd_bus_error_free(&err);
            return e;
        }
        const char* path = nullptr;
        r = sd_bus_message_read(reply, "o", &path);
        if (r >= 0 && path) device = path;
        sd_bus_message_unref(reply);
        if (device.empty()) return "fprintd returned no device";
        // Lost fprintd while an operation waits for signals.
        sd_bus_match_signal(bus, &owner_slot, "org.freedesktop.DBus", "/org/freedesktop/DBus",
                            "org.freedesktop.DBus", "NameOwnerChanged", &Fprint::on_owner, this);
        return {};
    }

    // Device method with zero or one string argument. Returns 0 or an fprintd error short name.
    std::string call(const char* method, const char* arg = nullptr, std::vector<std::string>* out_strings = nullptr)
    {
        sd_bus_error err = SD_BUS_ERROR_NULL;
        sd_bus_message* reply = nullptr;
        int r = arg ? sd_bus_call_method(bus, kService, device.c_str(), kDeviceIface, method, &err, &reply, "s", arg)
                    : sd_bus_call_method(bus, kService, device.c_str(), kDeviceIface, method, &err, &reply, "");
        if (r < 0)
        {
            std::string e = sd_bus_error_is_set(&err) ? short_error(err) : std::string(std::strerror(-r));
            if (!(e == "NoEnrolledPrints" && out_strings))
                log("%s(%s): %s%s%s", method, arg ? arg : "", e.c_str(), err.message ? " -- " : "",
                    err.message ? err.message : "");
            sd_bus_error_free(&err);
            return e;
        }
        if (out_strings)
        {
            if (sd_bus_message_enter_container(reply, 'a', "s") >= 0)
            {
                const char* s = nullptr;
                while (sd_bus_message_read(reply, "s", &s) > 0) out_strings->emplace_back(s);
                sd_bus_message_exit_container(reply);
            }
        }
        sd_bus_message_unref(reply);
        return {};
    }

    int num_enroll_stages()
    {
        sd_bus_error err = SD_BUS_ERROR_NULL;
        int32_t v = 0;
        int r = sd_bus_get_property_trivial(bus, kService, device.c_str(), kDeviceIface, "num-enroll-stages", &err, 'i', &v);
        sd_bus_error_free(&err);
        return (r >= 0 && v > 0) ? v : 20;
    }

    // Enrolled finger names; empty on NoEnrolledPrints. Returns an error or "".
    std::string list(const std::string& user, std::vector<std::string>& fingers)
    {
        fingers.clear();
        std::string e = call("ListEnrolledFingers", user.c_str(), &fingers);
        if (e == "NoEnrolledPrints") { fingers.clear(); return {}; }
        return e;
    }

    // Claim, retrying while another client holds the device, until cancelled or timeout.
    std::string claim(const std::string& user, const std::atomic<bool>& cancelled, int timeout_ms)
    {
        auto until = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms);
        for (;;)
        {
            std::string e = call("Claim", user.c_str());
            if (e.empty()) { claimed = true; return {}; }
            if (e != "AlreadyInUse" && e != "Internal") return e;   // Internal: "device is still starting"
            if (cancelled.load()) return "canceled";
            if (timeout_ms >= 0 && std::chrono::steady_clock::now() > until) return e;
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
        }
    }

    void release()
    {
        if (claimed) { call("Release"); claimed = false; }
    }

    void watch(const char* signal)
    {
        sd_bus_slot** slot = std::strcmp(signal, "EnrollStatus") == 0 ? &enroll_slot : &verify_slot;
        sd_bus_match_signal(bus, slot, kService, device.c_str(), kDeviceIface, signal, &Fprint::on_status, this);
    }

    // Waits up to ~250 ms for the next status; returns true if one arrived.
    bool next(Status& out)
    {
        for (int i = 0; i < 2; ++i)
        {
            while (sd_bus_process(bus, nullptr) > 0)
                if (status.fresh) break;
            if (status.fresh) { out = status; status.fresh = false; return true; }
            if (i == 0) sd_bus_wait(bus, 250000);
        }
        return false;
    }

    bool lost() const { return gone; }

private:
    static int on_status(sd_bus_message* m, void* ud, sd_bus_error*)
    {
        auto* self = static_cast<Fprint*>(ud);
        const char* result = nullptr;
        int done = 0;
        if (sd_bus_message_read(m, "sb", &result, &done) >= 0 && result)
            self->status = Status{result, done != 0, true};
        return 0;
    }

    static int on_owner(sd_bus_message* m, void* ud, sd_bus_error*)
    {
        auto* self = static_cast<Fprint*>(ud);
        const char *name = nullptr, *old_owner = nullptr, *new_owner = nullptr;
        if (sd_bus_message_read(m, "sss", &name, &old_owner, &new_owner) >= 0 && name &&
            std::strcmp(name, kService) == 0 && new_owner && !*new_owner)
            self->gone = true;
        return 0;
    }

    sd_bus* bus = nullptr;
    sd_bus_slot* enroll_slot = nullptr;
    sd_bus_slot* verify_slot = nullptr;
    sd_bus_slot* owner_slot = nullptr;
    std::string device;
    Status status;
    bool claimed = false;
    bool gone = false;
};

// Owner uid of the active session on seat0 (the session whose lock screen is shown).
bool active_seat_uid(uid_t& uid)
{
    sd_bus* bus = nullptr;
    const char* addr = std::getenv("FP6_BIOMETRYD_FPRINT_BUS");
    if (!addr || !*addr) addr = "unix:path=/run/dbus/system_bus_socket";
    int r = sd_bus_new(&bus);
    if (r >= 0) r = sd_bus_set_address(bus, addr);
    if (r >= 0) r = sd_bus_set_bus_client(bus, 1);
    if (r >= 0) r = sd_bus_start(bus);
    bool ok = false;
    if (r >= 0)
    {
        sd_bus_error err = SD_BUS_ERROR_NULL;
        sd_bus_message* reply = nullptr;
        r = sd_bus_get_property(bus, "org.freedesktop.login1", "/org/freedesktop/login1/seat/seat0",
                                "org.freedesktop.login1.Seat", "ActiveSession", &err, &reply, "(so)");
        const char *sid = nullptr, *spath = nullptr;
        std::string session_path;
        if (r >= 0 && sd_bus_message_read(reply, "(so)", &sid, &spath) >= 0 && spath) session_path = spath;
        if (reply) sd_bus_message_unref(reply);
        reply = nullptr;
        sd_bus_error_free(&err);
        if (!session_path.empty() && session_path != "/")
        {
            r = sd_bus_get_property(bus, "org.freedesktop.login1", session_path.c_str(),
                                    "org.freedesktop.login1.Session", "User", &err, &reply, "(uo)");
            uint32_t u = 0;
            const char* upath = nullptr;
            if (r >= 0 && sd_bus_message_read(reply, "(uo)", &u, &upath) >= 0) { uid = u; ok = true; }
            if (reply) sd_bus_message_unref(reply);
            sd_bus_error_free(&err);
        }
    }
    if (bus) sd_bus_flush_close_unref(bus);
    return ok;
}

// Enrolment hints for Lomiri's fingerprint visual (lomiri-settings-components
// Setup.qml -> FingerprintVisual): FingerprintReader::Hints::masks is a list of
// rectangles in the unit square; every segment of the drawn fingerprint that
// intersects one is filled (once; the renderer ignores already-filled ones).
// fprintd reports only "stage passed", not where the finger lay, so coverage
// grows from the centre outwards in proportion to the accepted samples.
biometry::Dictionary enrol_hints(double fraction, bool finger_present)
{
    constexpr int cols = 6, rows = 8;
    struct Tile { double d; int c, r; };
    static const std::vector<Tile> order = [] {
        std::vector<Tile> t;
        for (int r = 0; r < rows; ++r)
            for (int c = 0; c < cols; ++c)
            {
                double dx = (c + 0.5) / cols - 0.5, dy = (r + 0.5) / rows - 0.5;
                t.push_back({dx * dx + dy * dy * 0.6, c, r});   // slightly taller than wide
            }
        std::stable_sort(t.begin(), t.end(), [](const Tile& a, const Tile& b) { return a.d < b.d; });
        return t;
    }();
    std::size_t n = fraction >= 1.0 ? order.size()
                                     : static_cast<std::size_t>(fraction * order.size() + 0.5);
    std::vector<biometry::Variant> masks;
    for (std::size_t i = 0; i < n && i < order.size(); ++i)
    {
        const auto& t = order[i];
        biometry::Rectangle rect;
        rect.top_left = biometry::Point{static_cast<double>(t.c) / cols, static_cast<double>(t.r) / rows};
        rect.bottom_right = biometry::Point{static_cast<double>(t.c + 1) / cols, static_cast<double>(t.r + 1) / rows};
        masks.push_back(biometry::Variant::r(rect));
    }
    using Hints = biometry::devices::FingerprintReader::GuidedEnrollment::Hints;
    biometry::Dictionary d;
    d[Hints::key_masks] = biometry::Variant::v(masks);
    d[Hints::key_is_finger_present] = biometry::Variant::b(finger_present);
    return d;
}

// ---------------------------------------------------------------------------
// A biometry::Operation whose body runs on its own thread.
template <typename T>
class ThreadOp : public biometry::Operation<T>, public std::enable_shared_from_this<ThreadOp<T>>
{
public:
    using Observer = typename biometry::Operation<T>::Observer;
    using Body = std::function<void(ThreadOp&, const typename Observer::Ptr&)>;

    explicit ThreadOp(Body body) : body{std::move(body)} {}

    void start_with_observer(const typename Observer::Ptr& observer) override
    {
        if (started.exchange(true)) return;
        auto self = this->shared_from_this();
        std::thread([self, observer]() {
            observer->on_started();
            if (self->cancelled.load()) { observer->on_canceled("canceled"); return; }
            try
            {
                self->body(*self, observer);
            }
            catch (const std::exception& e)
            {
                log("operation failed: %s", e.what());
                observer->on_failed(e.what());
            }
        }).detach();
    }

    void cancel() override { cancelled.store(true); }

    std::atomic<bool> cancelled{false};

private:
    std::atomic<bool> started{false};
    Body body;
};

template <typename T>
typename biometry::Operation<T>::Ptr make_op(typename ThreadOp<T>::Body body)
{
    return std::make_shared<ThreadOp<T>>(std::move(body));
}

// Runs a verify for uid until a verdict or cancellation. Returns:
//   1 match, 0 no match, -1 canceled, -2 error (err set).
int run_verify(const std::atomic<bool>& cancelled, uid_t uid, std::string& err,
               const std::function<void(const std::string&)>& on_hint)
{
    std::string user = user_name(uid);
    if (user.empty()) { err = "unknown user"; return -2; }

    // fprintd not (yet) on the bus, still starting, or nothing enrolled: wait
    // quietly until it can verify or the caller cancels. These are not
    // verdicts, and the greeter counts every failure towards its lockout of
    // fingerprint unlock (failedFingerprintLogins), e.g. at boot when the
    // greeter starts before fingerprintd is ready.
    std::unique_ptr<Fprint> fpp;
    std::string last_wait;
    for (;;)
    {
        std::string why;
        auto candidate = std::make_unique<Fprint>();
        if (auto e = candidate->open(); !e.empty()) why = e;
        else
        {
            std::vector<std::string> fingers;
            if (auto e2 = candidate->list(user, fingers); !e2.empty()) why = e2;
            else if (fingers.empty()) why = "no fingers enrolled";
            else { fpp = std::move(candidate); break; }
        }
        if (why != last_wait) { log("identify waiting: %s", why.c_str()); last_wait = why; }
        for (int i = 0; i < 20 && !cancelled.load(); ++i) std::this_thread::sleep_for(std::chrono::milliseconds(100));
        if (cancelled.load()) return -1;
    }
    Fprint& fp = *fpp;

    if (auto e = fp.claim(user, cancelled, -1); !e.empty())
    {
        if (e == "canceled") return -1;
        err = e;
        return -2;
    }
    fp.watch("VerifyStatus");
    if (auto e = fp.call("VerifyStart", "any"); !e.empty()) { fp.release(); err = e; return -2; }

    int verdict = -1;
    Fprint::Status st;
    while (!cancelled.load())
    {
        if (fp.lost()) { err = "fprintd disappeared"; verdict = -2; break; }
        if (!fp.next(st)) continue;
        if (st.result == "verify-match") { verdict = 1; break; }
        if (st.result == "verify-no-match") { verdict = 0; break; }
        if (st.done) { err = st.result; verdict = -2; break; }
        on_hint(st.result);   // retry-scan, swipe-too-short, finger-not-centered, ...
    }
    if (!fp.lost())
    {
        fp.call("VerifyStop");
        fp.release();
    }
    return verdict;
}

// ---------------------------------------------------------------------------
class FprintTemplateStore : public biometry::TemplateStore
{
public:
    biometry::Operation<SizeQuery>::Ptr size(const biometry::Application&, const biometry::User& user) override
    {
        uid_t uid = user.id;
        return make_op<SizeQuery>([uid](ThreadOp<SizeQuery>&, const ThreadOp<SizeQuery>::Observer::Ptr& o) {
            std::vector<std::string> fingers;
            if (auto e = list_for(uid, fingers); !e.empty()) { o->on_failed(e); return; }
            o->on_succeeded(static_cast<std::uint32_t>(fingers.size()));
        });
    }

    biometry::Operation<List>::Ptr list(const biometry::Application&, const biometry::User& user) override
    {
        uid_t uid = user.id;
        return make_op<List>([uid](ThreadOp<List>&, const ThreadOp<List>::Observer::Ptr& o) {
            std::vector<std::string> fingers;
            if (auto e = list_for(uid, fingers); !e.empty()) { o->on_failed(e); return; }
            List::Result ids;
            for (const auto& f : fingers)
                if (int i = finger_index(f); i >= 0) ids.push_back(static_cast<TemplateId>(i + 1));
            std::sort(ids.begin(), ids.end());
            o->on_succeeded(ids);
        });
    }

    biometry::Operation<Enrollment>::Ptr enroll(const biometry::Application&, const biometry::User& user) override
    {
        uid_t uid = user.id;
        return make_op<Enrollment>([uid](ThreadOp<Enrollment>& op, const ThreadOp<Enrollment>::Observer::Ptr& o) {
            std::string name = user_name(uid);
            if (name.empty()) { o->on_failed("unknown user"); return; }
            Fprint fp;
            if (auto e = fp.open(); !e.empty()) { o->on_failed(e); return; }

            std::vector<std::string> fingers;
            if (auto e = fp.list(name, fingers); !e.empty()) { o->on_failed(e); return; }
            int slot = -1;
            for (int i : kEnrolOrder)
                if (std::find(fingers.begin(), fingers.end(), kFingers[i]) == fingers.end()) { slot = i; break; }
            if (slot < 0) { o->on_failed("enroll-data-full"); return; }

            if (auto e = fp.claim(name, op.cancelled, 5000); !e.empty())
            {
                if (e == "canceled") o->on_canceled("canceled"); else o->on_failed(e);
                return;
            }
            const int stages = fp.num_enroll_stages();
            fp.watch("EnrollStatus");
            if (auto e = fp.call("EnrollStart", kFingers[slot]); !e.empty())
            {
                fp.release();
                o->on_failed(e);
                return;
            }
            log("enrolment started for uid %u (slot %d, %d stages)", static_cast<unsigned>(uid), slot + 1, stages);
            o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(0.0), enrol_hints(0.0, false)});

            int passed = 0;
            enum { Running, Done, Failed, Canceled } state = Running;
            std::string why;
            Fprint::Status st;
            while (state == Running)
            {
                if (op.cancelled.load()) { state = Canceled; break; }
                if (fp.lost()) { state = Failed; why = "fprintd disappeared"; break; }
                if (!fp.next(st)) continue;
                if (st.result == "enroll-completed") { state = Done; break; }
                if (st.result == "enroll-stage-passed")
                {
                    ++passed;
                    double p = std::min(0.99, static_cast<double>(passed) / std::max(1, stages));
                    o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(p), enrol_hints(p, true)});
                }
                else if (st.done)
                {
                    state = Failed;
                    why = st.result;
                }
                else
                {
                    // retry-scan & co.: same progress, the page keeps prompting.
                    double p = std::min(0.99, static_cast<double>(passed) / std::max(1, stages));
                    o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(p), enrol_hints(p, false)});
                }
            }
            if (!fp.lost())
            {
                fp.call("EnrollStop");
                fp.release();
            }
            switch (state)
            {
            case Done:
                log("enrolment completed for uid %u (slot %d)", static_cast<unsigned>(uid), slot + 1);
                o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(1.0), enrol_hints(1.0, false)});
                o->on_succeeded(static_cast<TemplateId>(slot + 1));
                break;
            case Canceled:
                log("enrolment canceled for uid %u", static_cast<unsigned>(uid));
                o->on_canceled("canceled");
                break;
            default:
                log("enrolment failed for uid %u: %s", static_cast<unsigned>(uid), why.c_str());
                o->on_failed(why.empty() ? "enroll-failed" : why);
                break;
            }
        });
    }

    biometry::Operation<Removal>::Ptr remove(const biometry::Application&, const biometry::User& user, TemplateId id) override
    {
        uid_t uid = user.id;
        return make_op<Removal>([uid, id](ThreadOp<Removal>& op, const ThreadOp<Removal>::Observer::Ptr& o) {
            if (id < 1 || id > kFingers.size()) { o->on_failed("no such template"); return; }
            std::string name = user_name(uid);
            if (name.empty()) { o->on_failed("unknown user"); return; }
            Fprint fp;
            if (auto e = fp.open(); !e.empty()) { o->on_failed(e); return; }
            std::vector<std::string> fingers;
            if (auto e = fp.list(name, fingers); !e.empty()) { o->on_failed(e); return; }
            const char* finger = kFingers[id - 1];
            if (std::find(fingers.begin(), fingers.end(), finger) == fingers.end())
            {
                o->on_succeeded(id);   // already gone
                return;
            }
            if (auto e = fp.claim(name, op.cancelled, 5000); !e.empty()) { o->on_failed(e); return; }
            std::string e = fp.call("DeleteEnrolledFinger", finger);
            fp.release();
            if (!e.empty()) { o->on_failed(e); return; }
            log("removed template %llu for uid %u", static_cast<unsigned long long>(id), static_cast<unsigned>(uid));
            o->on_succeeded(id);
        });
    }

    biometry::Operation<Clearance>::Ptr clear(const biometry::Application&, const biometry::User& user) override
    {
        uid_t uid = user.id;
        return make_op<Clearance>([uid](ThreadOp<Clearance>&, const ThreadOp<Clearance>::Observer::Ptr& o) {
            std::string name = user_name(uid);
            if (name.empty()) { o->on_failed("unknown user"); return; }
            Fprint fp;
            if (auto e = fp.open(); !e.empty()) { o->on_failed(e); return; }
            std::string e = fp.call("DeleteEnrolledFingers", name.c_str());
            if (!e.empty() && e != "NoEnrolledPrints") { o->on_failed(e); return; }
            log("cleared all templates for uid %u", static_cast<unsigned>(uid));
            o->on_succeeded(biometry::Void{});
        });
    }

private:
    static std::string list_for(uid_t uid, std::vector<std::string>& fingers)
    {
        std::string name = user_name(uid);
        if (name.empty()) return "unknown user";
        Fprint fp;
        if (auto e = fp.open(); !e.empty()) return e;
        return fp.list(name, fingers);
    }
};

class FprintIdentifier : public biometry::Identifier
{
public:
    biometry::Operation<biometry::Identification>::Ptr identify_user(const biometry::Application&, const biometry::Reason&) override
    {
        using Id = biometry::Identification;
        return make_op<Id>([](ThreadOp<Id>& op, const ThreadOp<Id>::Observer::Ptr& o) {
            uid_t uid = 0;
            if (!active_seat_uid(uid)) { o->on_failed("no active session on seat0"); return; }
            std::string err;
            int v = run_verify(op.cancelled, uid, err, [&](const std::string&) {
                o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(0.0), biometry::Dictionary{}});
            });
            switch (v)
            {
            case 1: o->on_succeeded(biometry::User{uid}); break;
            case 0: o->on_failed("verify-no-match"); break;
            case -1: o->on_canceled("canceled"); break;
            default: o->on_failed(err.empty() ? "error" : err); break;
            }
        });
    }
};

class FprintVerifier : public biometry::Verifier
{
public:
    biometry::Operation<biometry::Verification>::Ptr verify_user(const biometry::Application&, const biometry::User& user, const biometry::Reason&) override
    {
        using Ver = biometry::Verification;
        uid_t uid = user.id;
        return make_op<Ver>([uid](ThreadOp<Ver>& op, const ThreadOp<Ver>::Observer::Ptr& o) {
            std::string err;
            int v = run_verify(op.cancelled, uid, err, [&](const std::string&) {
                o->on_progress(biometry::Progress{biometry::Percent::from_raw_value(0.0), biometry::Dictionary{}});
            });
            switch (v)
            {
            case 1: o->on_succeeded(Ver::Result::verified); break;
            case 0: o->on_succeeded(Ver::Result::not_verified); break;
            case -1: o->on_canceled("canceled"); break;
            default: o->on_failed(err.empty() ? "error" : err); break;
            }
        });
    }
};

class FprintDevice : public biometry::Device
{
public:
    biometry::TemplateStore& template_store() override { return store; }
    biometry::Identifier& identifier() override { return ident; }
    biometry::Verifier& verifier() override { return ver; }

private:
    FprintTemplateStore store;
    FprintIdentifier ident;
    FprintVerifier ver;
};
}  // namespace

BIOMETRYD_DEVICES_PLUGIN_DESCRIBE("fp6-fprintd", "FP6 NixOS", "biometryd device backed by fprintd (net.reactivated.Fprint)", 0, 1, 0)

BIOMETRYD_DEVICES_PLUGIN_CREATE
{
    log("loaded; backend net.reactivated.Fprint");
    return new FprintDevice();
}

BIOMETRYD_DEVICES_PLUGIN_DESTROY
{
    delete d;
}
