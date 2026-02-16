cmake_minimum_required(VERSION 4.1)
project(Gobol)

set(CMAKE_BUILD_TYPE Debug)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

file(GLOB SOURCE
    "${CMAKE_SOURCE_DIR}/*.cpp"
    "${CMAKE_SOURCE_DIR}/*.ixx"
)

add_executable(Gobol ${SOURCE})

target_include_directories(Gobol 
    PUBLIC ${CMAKE_SOURCE_DIR}
)

if (CMAKE_BUILD_TYPE MATCHES "^[Dd]ebug$")
    target_compile_definitions(Gobol 
        PRIVATE DEBUG
    )
endif ()

if (CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CMAKE_CXX_FLAGS_DEBUG "${CMAKE_CXX_FLAGS_DEBUG} -g -O0")
endif ()
