import QtQuick
import QtTest
import "../../modules/background/effects" as Effects

Item {
    width: 128
    height: 128

    Rectangle {
        id: wallpaper
        width: 128
        height: 128
        color: "red"
        visible: false
        property bool pattern: false
        Rectangle {
            x: 64
            width: 64
            height: 128
            color: "blue"
            visible: wallpaper.pattern
        }
    }

    Rectangle {
        id: otherWallpaper
        width: 128
        height: 128
        color: "green"
        visible: false
    }

    Effects.RetainedWallpaperBlur {
        id: blur
        width: 128
        height: 128
        sourceItem: wallpaper
    }

    TestCase {
        name: "RetainedWallpaperBlur"
        when: windowShown

        function init() {
            blur.retain = false
            blur.sourceItem = wallpaper
            blur.radius = 32
            blur.fadeProgress = 0
            blur.sourceAnimating = false
            blur.visible = true
            blur.width = 128
            blur.height = 128
            wallpaper.color = "red"
            wallpaper.pattern = false
            wallpaper.visible = false
            wallpaper.opacity = 1
            wallpaper.layer.enabled = false
            tryCompare(blur, "item", null)
        }

        function loadBlur() {
            blur.retain = true
            tryCompare(blur, "status", Loader.Ready)
            verify(blur.item !== null)
            return blur.item
        }

        function test_focusFadesReuseEffect() {
            const effect = loadBlur()
            compare(blur.opacity, 0)
            for (let i = 0; i < 10; i++) {
                blur.fadeProgress = 1
                compare(blur.item, effect)
                compare(blur.opacity, 1)
                blur.fadeProgress = 0.5
                compare(blur.opacity, 0.5)
                blur.fadeProgress = 0
                wait(1)
                compare(blur.item, effect)
                compare(effect.cached, true)
                compare(effect.source, wallpaper)
            }
        }

        function test_disableReleasesEffect() {
            loadBlur()
            blur.retain = false
            tryCompare(blur, "item", null)
            compare(blur.active, false)
        }

        function test_noSourceReleasesEffect() {
            loadBlur()
            blur.sourceItem = null
            tryCompare(blur, "item", null)
        }

        function test_zeroRadiusReleasesEffect() {
            loadBlur()
            blur.radius = 0
            tryCompare(blur, "item", null)
        }

        function test_animatedSourceBypassesCache() {
            const effect = loadBlur()
            compare(effect.cached, true)
            blur.sourceAnimating = true
            compare(effect.cached, false)
            compare(blur.item, effect)
            blur.sourceAnimating = false
            compare(effect.cached, true)
        }

        function test_settingsAndSizeUpdateInPlace() {
            const effect = loadBlur()
            blur.radius = 8
            compare(effect.radius, 8)
            compare(effect.samples, 17)
            blur.radius = 40
            compare(effect.samples, 33)
            blur.width = 96
            blur.height = 80
            compare(effect.width, 96)
            compare(effect.height, 80)
            compare(blur.item, effect)
        }

        function test_sourceReplacementUpdatesInPlace() {
            const effect = loadBlur()
            blur.sourceItem = otherWallpaper
            compare(effect.source, otherWallpaper)
            compare(blur.item, effect)
        }

        function test_hiddenLayerReusesEffect() {
            const effect = loadBlur()
            blur.visible = false
            wait(1)
            compare(blur.item, effect)
            blur.visible = true
            compare(blur.item, effect)
        }

        function test_cachedPixelsUpdateAfterHiddenSourceChange() {
            loadBlur()
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            tryVerify(function() { return Qt.colorEqual(grabImage(blur).pixel(64, 64), "red") })
            blur.fadeProgress = 0
            wallpaper.color = "blue"
            wait(20)
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            tryVerify(function() { return Qt.colorEqual(grabImage(blur).pixel(64, 64), "blue") })
        }

        function test_cachedPixelsUpdateAfterSourceReplacement() {
            loadBlur()
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            blur.sourceItem = otherWallpaper
            verify(waitForRendering(blur))
            tryVerify(function() { return Qt.colorEqual(grabImage(blur).pixel(64, 64), "green") })
        }

        function test_hiddenTextureProviderStillDrawsBlur() {
            // The awww path supplies a layer texture from an opacity-zero item.
            wallpaper.visible = true
            wallpaper.layer.enabled = true
            wallpaper.opacity = 0
            loadBlur()
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            tryVerify(function() { return Qt.colorEqual(grabImage(blur).pixel(64, 64), "red") })
        }

        function test_cachePreservesBlurPixels() {
            wallpaper.visible = true
            wallpaper.layer.enabled = true
            wallpaper.opacity = 0
            wallpaper.pattern = true
            loadBlur()
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            const cachedPixel = grabImage(blur).pixel(60, 64)
            verify(cachedPixel.r > 0 && cachedPixel.b > 0)
            blur.sourceAnimating = true
            verify(waitForRendering(blur))
            const livePixel = grabImage(blur).pixel(60, 64)
            fuzzyCompare(cachedPixel.r, livePixel.r, 0.01)
            fuzzyCompare(cachedPixel.b, livePixel.b, 0.01)
            fuzzyCompare(cachedPixel.a, livePixel.a, 0.01)
        }

        function test_radiusChangeRefreshesCachedPixels() {
            wallpaper.visible = true
            wallpaper.layer.enabled = true
            wallpaper.opacity = 0
            wallpaper.pattern = true
            blur.radius = 8
            loadBlur()
            blur.fadeProgress = 1
            verify(waitForRendering(blur))
            const narrow = grabImage(blur).pixel(54, 64).b
            blur.radius = 32
            verify(waitForRendering(blur))
            tryVerify(function() { return grabImage(blur).pixel(54, 64).b > narrow + 0.01 })
        }
    }
}
