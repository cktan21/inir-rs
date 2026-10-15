#include <QApplication>
#include <QLabel>
#include <QSocketNotifier>
#include <QTimer>
#include <cstdio>
#include <cstring>
#include <unistd.h>

// Paint only once; title changes exercise Niri and the shell, not our content.
// The harness owns this process and starts/stops the workload through stdin.
int main(int argc, char **argv) {
    QApplication app(argc, argv);
    app.setApplicationName("inir-title-churn");
    QLabel window("Backend comparison: static content, title-only updates.");
    window.resize(520, 140);
    window.setWindowTitle("INIR title fixture: ready");
    window.show();
    quint64 serial = 0;
    QTimer timer;
    timer.setTimerType(Qt::PreciseTimer);
    timer.setInterval(50);
    QObject::connect(&timer, &QTimer::timeout, [&] {
        window.setWindowTitle("INIR title fixture: " + QString::number(++serial));
    });
    QSocketNotifier input(STDIN_FILENO, QSocketNotifier::Read);
    QObject::connect(&input, &QSocketNotifier::activated, [&] {
        char line[32];
        if (!std::fgets(line, sizeof(line), stdin)) app.quit();
        else if (!std::strcmp(line, "start\n")) timer.start();
        else if (!std::strcmp(line, "stop\n")) timer.stop();
        else if (!std::strcmp(line, "quit\n")) app.quit();
    });
    return app.exec();
}
