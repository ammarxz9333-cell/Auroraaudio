idf_component_register(
    SRCS "aurora_esp_avb_evidence_espidf.c"
    INCLUDE_DIRS "include"
    REQUIRES esp_avb esp_ptp
)
