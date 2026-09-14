#include <errno.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>

#include "saf.h"

static int read_directions(const char *path, float **directions, int *count) {
    FILE *file = fopen(path, "r");
    if (file == NULL) {
        fprintf(stderr, "failed to open %s\n", path);
        return 0;
    }

    int n = 0;
    if (fscanf(file, "%d", &n) != 1 || n <= 0) {
        fprintf(stderr, "invalid direction count in %s\n", path);
        fclose(file);
        return 0;
    }

    float *values = (float *)calloc((size_t)n * 2U, sizeof(float));
    if (values == NULL) {
        fprintf(stderr, "allocation failed for %s\n", path);
        fclose(file);
        return 0;
    }

    for (int index = 0; index < n; ++index) {
        float azimuth = 0.0f;
        float elevation = 0.0f;
        if (fscanf(file, "%f %f", &azimuth, &elevation) != 2 ||
            !isfinite(azimuth) || !isfinite(elevation)) {
            fprintf(stderr, "invalid direction %d in %s\n", index, path);
            free(values);
            fclose(file);
            return 0;
        }
        values[index * 2] = azimuth;
        values[index * 2 + 1] = elevation;
    }

    fclose(file);
    *directions = values;
    *count = n;
    return 1;
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s SPEAKERS.txt SOURCES.txt OUTPUT.txt\n", argv[0]);
        return 2;
    }

    float *speaker_directions = NULL;
    float *source_directions = NULL;
    float *gain_table = NULL;
    int speaker_count = 0;
    int source_count = 0;
    int gain_rows = 0;
    int triangle_count = 0;
    int result = 1;

    if (!read_directions(argv[1], &speaker_directions, &speaker_count) ||
        !read_directions(argv[2], &source_directions, &source_count)) {
        goto cleanup;
    }

    /*
     * Use only the real Aurora loudspeaker directions: no dummy speakers and
     * no large-triangle pruning. LFE is removed by the fixture-preparation
     * script before this probe is invoked.
     */
    generateVBAPgainTable3D_srcs(source_directions,
                                 source_count,
                                 speaker_directions,
                                 speaker_count,
                                 0,
                                 0,
                                 0.0f,
                                 &gain_table,
                                 &gain_rows,
                                 &triangle_count);

    if (gain_table == NULL || gain_rows != source_count || triangle_count <= 0) {
        fprintf(stderr,
                "SAF triangulation failed: gains=%p rows=%d sources=%d triangles=%d\n",
                (void *)gain_table,
                gain_rows,
                source_count,
                triangle_count);
        goto cleanup;
    }

    FILE *output = fopen(argv[3], "w");
    if (output == NULL) {
        fprintf(stderr, "failed to open %s: errno=%d\n", argv[3], errno);
        goto cleanup;
    }

    fprintf(output, "%d %d %d\n", gain_rows, speaker_count, triangle_count);
    for (int row = 0; row < gain_rows; ++row) {
        double power = 0.0;
        for (int speaker = 0; speaker < speaker_count; ++speaker) {
            const float gain = gain_table[row * speaker_count + speaker];
            if (!isfinite(gain) || gain < -1.0e-6f) {
                fprintf(stderr, "non-finite/negative SAF gain at row=%d speaker=%d\n", row, speaker);
                fclose(output);
                goto cleanup;
            }
            power += (double)gain * (double)gain;
            fprintf(output, speaker == 0 ? "%.9g" : " %.9g", gain);
        }
        fprintf(output, "\n");
        if (!isfinite(power) || fabs(power - 1.0) > 2.0e-3) {
            fprintf(stderr, "SAF row %d is not energy-normalized: %.9f\n", row, power);
            fclose(output);
            goto cleanup;
        }
    }

    if (fclose(output) != 0) {
        fprintf(stderr, "failed to close output %s\n", argv[3]);
        goto cleanup;
    }

    result = 0;

cleanup:
    free(gain_table);
    free(source_directions);
    free(speaker_directions);
    return result;
}
