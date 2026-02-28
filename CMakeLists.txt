cmake_minimum_required(VERSION 4.1)
project(Gobol)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

# 干掉注释警告

if(CMAKE_CXX_COMPILER_ID MATCHES "Clang|GNU")
    add_compile_options(-Wno-comment)
elseif(MSVC)
    add_compile_options(/wd4138)
endif()

file(GLOB SOURCE
    "${CMAKE_SOURCE_DIR}/*/*.cpp"
    "${CMAKE_SOURCE_DIR}/*/*.hpp"
    "${CMAKE_SOURCE_DIR}/main.cpp"
)

add_executable(gobol ${SOURCE})

target_include_directories(gobol 
    PUBLIC ${CMAKE_SOURCE_DIR}
)

if (CMAKE_BUILD_TYPE MATCHES "^[Dd]ebug$")
    target_compile_definitions(gobol 
        PUBLIC DEBUG
    )
    
endif ()

if (CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(CMAKE_CXX_FLAGS_DEBUG "${CMAKE_CXX_FLAGS_DEBUG} -g -O0")
endif ()
