// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "FuzzEnvironment.hpp"
#include "io/WawaV10Importer.hpp"

#include <cstddef>
#include <cstdint>

extern "C" int LLVMFuzzerInitialize(int *argc, char ***argv)
{
    ugurugu::fuzzing::initialize(argc, argv);
    return 0;
}

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size)
{
    ugurugu::WawaV10Importer::import(ugurugu::fuzzing::toByteArray(data, size));
    return 0;
}
