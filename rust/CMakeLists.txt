cmake_minimum_required(VERSION 3.24)
project(inir_native VERSION 0.1.0 LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
include(GNUInstallDirs)
include(CTest)
find_package(Qt6 6.5 REQUIRED COMPONENTS Core Qml)

# Pinned build helpers are vendored so distribution builds never fetch CMake
# code from the network. Cargo dependencies are pinned by Cargo.lock.
set(FETCHCONTENT_SOURCE_DIR_CORROSION "${CMAKE_CURRENT_SOURCE_DIR}/cmake/vendor/corrosion")
add_subdirectory(cmake/vendor/cxx-qt EXCLUDE_FROM_ALL)
cxx_qt_import_crate(
    MANIFEST_PATH "${CMAKE_CURRENT_SOURCE_DIR}/Cargo.toml"
    CRATES inir-qt LOCKED
    QT_MODULES Qt6::Core Qt6::Qml
)
cxx_qt_import_qml_module(inir_native_qml
    URI "qs.services.native" SOURCE_CRATE inir_qt
    OUTPUT_DIR "${CMAKE_CURRENT_BINARY_DIR}/qml")

# Also refresh the staged module after every Cargo build, including incremental
# builds: upstream's copy rule only checks whether qmldir exists.
get_target_property(INIR_CXX_QT_EXPORT_DIR inir_qt CXX_QT_EXPORT_DIR)
add_custom_target(inir_native_stage ALL
    COMMAND "${CMAKE_COMMAND}" -E copy_directory_if_different
        "${INIR_CXX_QT_EXPORT_DIR}/qml_modules/qs/services/native"
        "${CMAKE_CURRENT_BINARY_DIR}/qml/qs/services/native"
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different
        "$<TARGET_FILE:inir_qt-shared>"
        "${CMAKE_CURRENT_BINARY_DIR}/qml/qs/services/native/libqs_services_native.so"
    DEPENDS inir_native_qml inir_qt)

set(INIR_QML_INSTALL_DIR "${CMAKE_INSTALL_LIBDIR}/qt-6/qml" CACHE PATH "QML module import root")
install(DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/qml/qs"
    DESTINATION "${INIR_QML_INSTALL_DIR}")

if(BUILD_TESTING)
    find_package(Qt6 REQUIRED COMPONENTS Test)
    set(CMAKE_AUTOMOC ON)
    add_executable(inir_service_models tests/service_models.cpp inir-qt/src/service_models.cpp inir-qt/include/service_models.h)
    target_include_directories(inir_service_models PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/model-include")
    file(MAKE_DIRECTORY "${CMAKE_CURRENT_BINARY_DIR}/model-include/inir-qt")
    configure_file(inir-qt/include/service_models.h model-include/inir-qt/service_models.h COPYONLY)
    target_link_libraries(inir_service_models PRIVATE Qt6::Core Qt6::Test)
    add_test(NAME native_service_models COMMAND inir_service_models)
    add_executable(inir_bridge_smoke tests/bridge_smoke.cpp)
    target_link_libraries(inir_bridge_smoke PRIVATE Qt6::Core Qt6::Qml)
    add_dependencies(inir_bridge_smoke inir_native_stage)
    add_test(NAME native_bridge COMMAND inir_bridge_smoke "${CMAKE_CURRENT_BINARY_DIR}/qml")
    set_tests_properties(native_bridge PROPERTIES TIMEOUT 30)
endif()
