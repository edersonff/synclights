cmake_minimum_required(VERSION 3.22)
project(SyncLights VERSION 0.1 LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)
find_package(Qt6 REQUIRED COMPONENTS Widgets)
add_executable(synclights-desktop desktop/main.cpp assets.qrc)
target_link_libraries(synclights-desktop PRIVATE Qt6::Widgets)
target_compile_options(synclights-desktop PRIVATE -O2 -Wall -Wextra)
target_compile_definitions(synclights-desktop PRIVATE SYNCLIGHTS_ICON=":/synclights-icon.png")
add_custom_target(synclights-engine ALL
  COMMAND cargo build --release --offline --manifest-path ${CMAKE_SOURCE_DIR}/Cargo.toml
  COMMAND ${CMAKE_COMMAND} -E copy ${CMAKE_SOURCE_DIR}/target/release/synclights $<TARGET_FILE_DIR:synclights-desktop>/synclights
  WORKING_DIRECTORY ${CMAKE_SOURCE_DIR}
  COMMENT "Building and bundling the Rust screen-sync engine")
add_dependencies(synclights-desktop synclights-engine)
install(TARGETS synclights-desktop RUNTIME DESTINATION bin)
install(FILES ${CMAKE_SOURCE_DIR}/target/release/synclights DESTINATION bin PERMISSIONS OWNER_READ OWNER_WRITE OWNER_EXECUTE GROUP_READ GROUP_EXECUTE WORLD_READ WORLD_EXECUTE)
install(FILES ${CMAKE_SOURCE_DIR}/assets/synclights-icon-256.png DESTINATION share/icons/hicolor/256x256/apps RENAME synclights.png)
install(FILES ${CMAKE_SOURCE_DIR}/desktop/synclights.desktop DESTINATION share/applications)
