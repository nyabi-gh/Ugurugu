// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Nyabi (nyabi-gh)

#pragma once

namespace ugurugu
{

int runAppPolicyTests(int argc, char **argv);
int runDocumentTests(int argc, char **argv);
int runRenderEngineTests(int argc, char **argv);
int runGifWriterTests(int argc, char **argv);
int runWebPWriterTests(int argc, char **argv);
int runMaskRegressionTests(int argc, char **argv);
int runReleaseNotesTests(int argc, char **argv);
int runStrokeStabilizerTests(int argc, char **argv);

}
