// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#include "TestSuites.hpp"
#include "support/DocumentTestSuites.hpp"

namespace ugurugu
{

int runDocumentTests(int argc, char **argv)
{
    int result = 0;
    result |= runDocumentLifecycleTests(argc, argv);
    result |= runDocumentHistoryTests(argc, argv);
    result |= runLayerCommandTests(argc, argv);
    result |= runDocumentSchemaTests(argc, argv);
    result |= runDocumentResizeTests(argc, argv);
    result |= runStrokeCommandTests(argc, argv);
    result |= runSelectionClipboardTests(argc, argv);
    result |= runTextStrokeBuilderTests(argc, argv);
    result |= runSerializationBudgetTests(argc, argv);
    result |= runRasterAssetTableTests(argc, argv);
    result |= runWawaV10ReaderTests(argc, argv);
    return result;
}

}
