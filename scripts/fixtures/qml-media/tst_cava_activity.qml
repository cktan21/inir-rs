import QtQuick
import QtTest
import "."

Item {
    width: 128
    height: 64
    CavaSpectrum {
        id: spectrum
        anchors.fill: parent
        active: true
        threadedRendering: false
        points: [20, 40, 80, 50]
        normalizationCeiling: 100
    }
    SignalSpy { id: paints; target: spectrum; signalName: "paint" }
    TestCase {
        name: "CavaDrawingOwnership"
        when: windowShown

        function init() {
            spectrum.drawing = true
            spectrum.active = true
            spectrum.visible = true
            spectrum.points = [20, 40, 80, 50]
            spectrum.normalizationCeiling = 100
            verify(waitForRendering(spectrum))
            wait(50)
            paints.clear()
        }
        function test_nonDrawingCopyDoesNotQueuePaints() {
            spectrum.drawing = false
            for (let i = 0; i < 20; i++) {
                spectrum.points = [i, 40, 80, 50]
                spectrum.normalizationCeiling = 100 - i
            }
            wait(100)
            compare(paints.count, 0)
        }
        function test_reactivationPaintsCurrentFrame() {
            spectrum.drawing = false
            spectrum.points = [90, 90, 90, 90]
            spectrum.drawing = true
            tryVerify(function() { return paints.count > 0 })
        }
        function test_drawingCopyStillPaintsProducerUpdates() {
            spectrum.points = [90, 90, 90, 90]
            tryVerify(function() { return paints.count > 0 })
        }
        function test_hiddenCopyDoesNotPaint() {
            spectrum.visible = false
            spectrum.points = [90, 90, 90, 90]
            wait(100)
            compare(paints.count, 0)
        }
        function test_retainedCopyPrimesAfterResize() {
            spectrum.drawing = false
            spectrum.width = 96
            tryVerify(function() { return paints.count > 0 })
            wait(50)
            paints.clear()
            spectrum.points = [80, 80, 80, 80]
            wait(100)
            compare(paints.count, 0)
            spectrum.anchors.fill = spectrum.parent
        }
    }
}
