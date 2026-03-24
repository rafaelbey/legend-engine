// Copyright 2026 Goldman Sachs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

package org.finos.legend.engine.language.pure.grammar.from;

import java.io.File;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;

public class RustPureParser
{
    static
    {
        try
        {
            // First, see if we are in a development environment where java.library.path is explicitly set
            System.loadLibrary("legend_pure_parser_jni");
        }
        catch (UnsatisfiedLinkError e)
        {
            // If not found, attempt to unpack from classpath resources for distribution compatibility
            try
            {
                String osName = System.getProperty("os.name").toLowerCase();
                String extension;
                String prefix = "lib";

                if (osName.contains("win"))
                {
                    extension = ".dll";
                    prefix = "";
                } else if (osName.contains("mac"))
                {
                    extension = ".dylib";
                } else
                {
                    extension = ".so";
                }

                String libraryName = prefix + "legend_pure_parser_jni" + extension;
                String resourcePath = "/native/" + libraryName;

                InputStream in = RustPureParser.class.getResourceAsStream(resourcePath);
                if (in == null)
                {
                    throw new RuntimeException("Failed to find native library in classpath: " + resourcePath);
                }

                File tempLib = File.createTempFile("legend_pure_parser_jni", extension);
                tempLib.deleteOnExit();

                Files.copy(in, tempLib.toPath(), StandardCopyOption.REPLACE_EXISTING);
                in.close();

                System.load(tempLib.getAbsolutePath());
            }
            catch (Exception ex)
            {
                System.err.println("Failed to completely load RustPureParser JNI layer, parser feature flag should remain false. \n" + ex);
                throw new RuntimeException(ex);
            }
        }
    }

    public native String parse(String input);
}
