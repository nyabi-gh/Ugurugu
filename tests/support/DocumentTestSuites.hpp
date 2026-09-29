// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

namespace ugurugu
{

int runDocumentLifecycleTests(int argc, char **argv);
int runDocumentHistoryTests(int argc, char **argv);
int runLayerCommandTests(int argc, char **argv);
int runDocumentSchemaTests(int argc, char **argv);
int runDocumentResizeTests(int argc, char **argv);
int runStrokeCommandTests(int argc, char **argv);
int runSelectionClipboardTests(int argc, char **argv);
int runTextStrokeBuilderTests(int argc, char **argv);
int runSerializationBudgetTests(int argc, char **argv);
int runRasterAssetTableTests(int argc, char **argv);
int runWawaV10ReaderTests(int argc, char **argv);

}
